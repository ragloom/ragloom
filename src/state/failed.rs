//! Failed-work journal for exhausted ingest items.
//!
//! # Why
//! Exhausted work should be inspectable and operator-replayable without
//! changing the WAL's acknowledgement/replay semantics.
//!
//! The journal shares the WAL's fail-closed compatibility posture: released
//! `v0.4.1+` newline-delimited JSON records remain directly readable through v1, while
//! unknown future variants or malformed lines fail with `state` context.

use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::error::{RagloomError, RagloomErrorKind};
use crate::state::wal::WalRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailedWorkFailureKind {
    InvalidInput,
    Io,
    Config,
    Internal,
    Embed,
    Sink,
    State,
}

impl FailedWorkFailureKind {
    pub fn from_error_kind(kind: RagloomErrorKind) -> Self {
        match kind {
            RagloomErrorKind::InvalidInput => Self::InvalidInput,
            RagloomErrorKind::Io => Self::Io,
            RagloomErrorKind::Config => Self::Config,
            RagloomErrorKind::Internal => Self::Internal,
            RagloomErrorKind::Embed => Self::Embed,
            RagloomErrorKind::Sink => Self::Sink,
            RagloomErrorKind::State => Self::State,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailedWorkTerminalReason {
    RetryExhausted,
    NonRetryable,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FailedWorkRecord {
    Exhausted {
        id: u64,
        work: WalRecord,
        failure_kind: FailedWorkFailureKind,
        terminal_reason: FailedWorkTerminalReason,
        attempts: u32,
    },
    Requeued {
        exhausted_id: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingFailedWork {
    pub id: u64,
    pub work: WalRecord,
    pub failure_kind: FailedWorkFailureKind,
    pub terminal_reason: FailedWorkTerminalReason,
    pub attempts: u32,
}

pub trait FailedWorkStore: Send {
    fn append(&mut self, record: FailedWorkRecord) -> Result<(), RagloomError>;
    fn read_all(&self) -> Result<Vec<FailedWorkRecord>, RagloomError>;
    fn is_empty(&self) -> bool;
}

#[derive(Clone)]
pub struct FailedWorkJournal {
    inner: std::sync::Arc<tokio::sync::Mutex<Box<dyn FailedWorkStore>>>,
}

impl std::fmt::Debug for FailedWorkJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FailedWorkJournal").finish_non_exhaustive()
    }
}

impl FailedWorkJournal {
    pub fn new<S: FailedWorkStore + 'static>(store: S) -> Self {
        Self {
            inner: std::sync::Arc::new(tokio::sync::Mutex::new(Box::new(store))),
        }
    }

    pub async fn append(&self, record: FailedWorkRecord) -> Result<(), RagloomError> {
        let mut guard = self.inner.lock().await;
        guard.append(record)
    }

    pub async fn append_exhausted(
        &self,
        work: WalRecord,
        failure_kind: FailedWorkFailureKind,
        terminal_reason: FailedWorkTerminalReason,
        attempts: u32,
    ) -> Result<u64, RagloomError> {
        let mut guard = self.inner.lock().await;
        let id = next_failed_work_id(&guard.read_all()?);
        guard.append(FailedWorkRecord::Exhausted {
            id,
            work,
            failure_kind,
            terminal_reason,
            attempts,
        })?;
        Ok(id)
    }

    pub async fn read_all(&self) -> Result<Vec<FailedWorkRecord>, RagloomError> {
        let guard = self.inner.lock().await;
        guard.read_all()
    }
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryFailedWorkStore {
    records: Vec<FailedWorkRecord>,
}

impl InMemoryFailedWorkStore {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    pub fn append(&mut self, record: FailedWorkRecord) -> Result<(), RagloomError> {
        <Self as FailedWorkStore>::append(self, record)
    }

    pub fn read_all(&self) -> Result<Vec<FailedWorkRecord>, RagloomError> {
        <Self as FailedWorkStore>::read_all(self)
    }

    pub fn is_empty(&self) -> bool {
        <Self as FailedWorkStore>::is_empty(self)
    }
}

impl FailedWorkStore for InMemoryFailedWorkStore {
    fn append(&mut self, record: FailedWorkRecord) -> Result<(), RagloomError> {
        self.records.push(record);
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<FailedWorkRecord>, RagloomError> {
        Ok(self.records.clone())
    }

    fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[derive(Debug)]
pub struct FileFailedWorkStore {
    path: PathBuf,
    file: File,
}

impl FileFailedWorkStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RagloomError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                    "failed to create failed-work directory: {}",
                    parent.display()
                ))
            })?;
        }

        let store = Self {
            file: open_failed_work_append_file(&path)?,
            path,
        };
        store.read_all().map_err(|e| {
            RagloomError::new(e.kind, e).with_context("failed to validate failed-work file")
        })?;
        Ok(store)
    }

    pub fn append(&mut self, record: FailedWorkRecord) -> Result<(), RagloomError> {
        <Self as FailedWorkStore>::append(self, record)
    }

    pub fn read_all(&self) -> Result<Vec<FailedWorkRecord>, RagloomError> {
        <Self as FailedWorkStore>::read_all(self)
    }

    pub fn is_empty(&self) -> bool {
        <Self as FailedWorkStore>::is_empty(self)
    }
}

impl FailedWorkStore for FileFailedWorkStore {
    fn append(&mut self, record: FailedWorkRecord) -> Result<(), RagloomError> {
        let encoded = serde_json::to_string(&record).map_err(|e| {
            RagloomError::new(RagloomErrorKind::State, e)
                .with_context("failed to encode failed-work record")
        })?;

        writeln!(self.file, "{encoded}").map_err(|e| {
            RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                "failed to append failed-work file: {}",
                self.path.display()
            ))
        })?;
        self.file.sync_data().map_err(|e| {
            RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                "failed to sync failed-work file: {}",
                self.path.display()
            ))
        })?;
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<FailedWorkRecord>, RagloomError> {
        let contents = std::fs::read_to_string(&self.path).map_err(|e| {
            RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                "failed to read failed-work file: {}",
                self.path.display()
            ))
        })?;

        let mut records = Vec::new();
        for (idx, line) in contents.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }

            let record = serde_json::from_str::<FailedWorkRecord>(line).map_err(|e| {
                RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                    "failed to parse failed-work record at line {} in {}",
                    idx + 1,
                    self.path.display()
                ))
            })?;
            records.push(record);
        }
        Ok(records)
    }

    fn is_empty(&self) -> bool {
        match std::fs::metadata(&self.path) {
            Ok(metadata) => metadata.len() == 0,
            Err(err) if err.kind() == ErrorKind::NotFound => true,
            Err(err) => {
                tracing::warn!(
                    event.name = "ragloom.failed_work.metadata_failed",
                    path = %self.path.display(),
                    error.message = %err,
                    "ragloom.failed_work.metadata_failed"
                );
                false
            }
        }
    }
}

pub fn pending_failed_work(records: &[FailedWorkRecord]) -> Vec<PendingFailedWork> {
    let mut pending = std::collections::BTreeMap::<u64, PendingFailedWork>::new();

    for record in records {
        match record {
            FailedWorkRecord::Exhausted {
                id,
                work,
                failure_kind,
                terminal_reason,
                attempts,
            } => {
                pending.insert(
                    *id,
                    PendingFailedWork {
                        id: *id,
                        work: work.clone(),
                        failure_kind: *failure_kind,
                        terminal_reason: *terminal_reason,
                        attempts: *attempts,
                    },
                );
            }
            FailedWorkRecord::Requeued { exhausted_id } => {
                pending.remove(exhausted_id);
            }
        }
    }

    pending.into_values().collect()
}

pub fn next_failed_work_id(records: &[FailedWorkRecord]) -> u64 {
    records
        .iter()
        .filter_map(|record| match record {
            FailedWorkRecord::Exhausted { id, .. } => Some(*id),
            FailedWorkRecord::Requeued { .. } => None,
        })
        .max()
        .unwrap_or(0)
        + 1
}

pub fn failed_work_path_from_state_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join("failed.ndjson"))
        .unwrap_or_else(|| PathBuf::from("failed.ndjson"))
}

fn open_failed_work_append_file(path: &Path) -> Result<File, RagloomError> {
    match std::fs::OpenOptions::new()
        .create_new(true)
        .append(true)
        .open(path)
    {
        Ok(file) => {
            sync_failed_work_parent_dir(path)?;
            Ok(file)
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists => std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                    "failed to open failed-work file: {}",
                    path.display()
                ))
            }),
        Err(err) => Err(
            RagloomError::new(RagloomErrorKind::State, err).with_context(format!(
                "failed to open failed-work file: {}",
                path.display()
            )),
        ),
    }
}

fn sync_failed_work_parent_dir(path: &Path) -> Result<(), RagloomError> {
    #[cfg(unix)]
    {
        let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        else {
            return Ok(());
        };
        File::open(parent)
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                    "failed to open failed-work parent directory: {}",
                    parent.display()
                ))
            })?
            .sync_all()
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                    "failed to sync failed-work parent directory: {}",
                    parent.display()
                ))
            })?;
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::FileFingerprint;
    use tempfile::{NamedTempFile, tempdir};

    fn sample_work(path: &str) -> WalRecord {
        WalRecord::WorkItemV2 {
            fingerprint: FileFingerprint {
                canonical_path: path.to_string(),
                size_bytes: 10,
                mtime_unix_secs: 100,
                etag: None,
            },
        }
    }

    #[test]
    fn file_failed_work_roundtrips_records_after_reopen() {
        let file = NamedTempFile::new().expect("temp failed-work");
        let path = file.path().to_path_buf();

        let mut store = FileFailedWorkStore::open(&path).expect("open");
        store
            .append(FailedWorkRecord::Exhausted {
                id: 1,
                work: sample_work("/x/a.txt"),
                failure_kind: FailedWorkFailureKind::Embed,
                terminal_reason: FailedWorkTerminalReason::RetryExhausted,
                attempts: 3,
            })
            .expect("append exhausted");
        store
            .append(FailedWorkRecord::Requeued { exhausted_id: 1 })
            .expect("append requeued");

        let reopened = FileFailedWorkStore::open(&path).expect("reopen");
        assert_eq!(
            reopened.read_all().expect("read"),
            vec![
                FailedWorkRecord::Exhausted {
                    id: 1,
                    work: sample_work("/x/a.txt"),
                    failure_kind: FailedWorkFailureKind::Embed,
                    terminal_reason: FailedWorkTerminalReason::RetryExhausted,
                    attempts: 3,
                },
                FailedWorkRecord::Requeued { exhausted_id: 1 },
            ]
        );
    }

    #[test]
    fn pending_failed_work_filters_requeued_entries() {
        let records = vec![
            FailedWorkRecord::Exhausted {
                id: 1,
                work: sample_work("/x/a.txt"),
                failure_kind: FailedWorkFailureKind::Sink,
                terminal_reason: FailedWorkTerminalReason::RetryExhausted,
                attempts: 2,
            },
            FailedWorkRecord::Exhausted {
                id: 2,
                work: sample_work("/x/b.txt"),
                failure_kind: FailedWorkFailureKind::Embed,
                terminal_reason: FailedWorkTerminalReason::NonRetryable,
                attempts: 1,
            },
            FailedWorkRecord::Requeued { exhausted_id: 1 },
        ];

        assert_eq!(
            pending_failed_work(&records),
            vec![PendingFailedWork {
                id: 2,
                work: sample_work("/x/b.txt"),
                failure_kind: FailedWorkFailureKind::Embed,
                terminal_reason: FailedWorkTerminalReason::NonRetryable,
                attempts: 1,
            }]
        );
    }

    #[test]
    fn later_exhaustion_after_prior_requeue_stays_pending() {
        let records = vec![
            FailedWorkRecord::Exhausted {
                id: 1,
                work: sample_work("/x/a.txt"),
                failure_kind: FailedWorkFailureKind::Io,
                terminal_reason: FailedWorkTerminalReason::RetryExhausted,
                attempts: 2,
            },
            FailedWorkRecord::Requeued { exhausted_id: 1 },
            FailedWorkRecord::Exhausted {
                id: 2,
                work: sample_work("/x/a.txt"),
                failure_kind: FailedWorkFailureKind::Io,
                terminal_reason: FailedWorkTerminalReason::RetryExhausted,
                attempts: 2,
            },
        ];

        assert_eq!(pending_failed_work(&records).len(), 1);
        assert_eq!(pending_failed_work(&records)[0].id, 2);
    }

    #[test]
    fn invalid_failed_work_file_fails_open_with_state_context() {
        let mut file = NamedTempFile::new().expect("temp failed-work");
        file.write_all(b"{not json}\n").expect("write");

        let err = FileFailedWorkStore::open(file.path()).expect_err("invalid store should fail");
        assert_eq!(err.kind, RagloomErrorKind::State);
        assert!(
            err.to_string()
                .contains("failed to validate failed-work file")
        );
        let source = std::error::Error::source(&err).expect("source");
        assert!(
            source
                .to_string()
                .contains("failed to parse failed-work record")
        );
    }

    #[test]
    fn failed_work_path_uses_state_directory() {
        assert_eq!(
            failed_work_path_from_state_path(".ragloom/wal.ndjson"),
            PathBuf::from(".ragloom").join("failed.ndjson")
        );
    }

    #[tokio::test]
    async fn failed_work_journal_allocates_unique_ids_under_concurrency() {
        let journal = FailedWorkJournal::new(InMemoryFailedWorkStore::new());
        let mut tasks = Vec::new();

        for idx in 0..8u64 {
            let journal = journal.clone();
            tasks.push(tokio::spawn(async move {
                journal
                    .append_exhausted(
                        sample_work(&format!("/x/{idx}.txt")),
                        FailedWorkFailureKind::Embed,
                        FailedWorkTerminalReason::RetryExhausted,
                        2,
                    )
                    .await
                    .expect("append exhausted")
            }));
        }

        let mut ids = Vec::new();
        for task in tasks {
            ids.push(task.await.expect("task join"));
        }

        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6, 7, 8]);

        let pending = pending_failed_work(&journal.read_all().await.expect("read records"));
        assert_eq!(pending.len(), 8);
        assert_eq!(
            pending.iter().map(|record| record.id).collect::<Vec<_>>(),
            ids
        );
    }

    #[test]
    fn file_failed_work_open_creates_missing_file() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("failed.ndjson");

        let store = FileFailedWorkStore::open(&path).expect("open new store");
        assert!(path.exists());
        assert!(store.is_empty());
    }

    #[test]
    fn failed_work_path_without_parent_skips_parent_sync_requirement() {
        let path = Path::new("failed.ndjson");
        sync_failed_work_parent_dir(path).expect("sync current-dir file parent should noop");
    }
}
