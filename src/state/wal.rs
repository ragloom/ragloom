//! Write-ahead log (WAL) record types.
//!
//! # Why
//! We rely on at-least-once execution with deterministic IDs. A minimal WAL lets
//! us replay un-acked work after crashes without requiring complex distributed
//! coordination.
//!
//! The on-disk contract is intentionally small and fail-closed: released
//! `v0.4.x` and `v0.5.0` newline-delimited JSON records stay directly readable, while
//! unknown future variants or malformed lines fail with `state` context rather
//! than being skipped.

use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::error::{RagloomError, RagloomErrorKind};

/// A durable record representing progress at the pipeline boundaries.
///
/// # Why
/// Only boundary events (enqueue work, acknowledge sink write) need to be
/// persisted to enable replay.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WalRecord {
    /// A chunk has been enqueued for processing.
    WorkItem { chunk_id: [u8; 32] },

    /// A file-derived work item has been enqueued for processing.
    ///
    /// # Why
    /// The MVP ingestion path identifies work by filesystem metadata rather than
    /// chunk IDs so we can reconstruct deterministic identifiers after restarts.
    WorkItemV2 {
        fingerprint: crate::ids::FileFingerprint,
    },

    /// A chunk has been successfully written to the sink.
    SinkAck { chunk_id: [u8; 32] },

    /// A file-derived work item has been successfully written to the sink.
    ///
    /// # Why
    /// `WorkItemV2` does not carry a chunk ID. We acknowledge completion using
    /// the same identity used to schedule work (the file fingerprint), keeping
    /// the WAL self-contained and replay-safe.
    SinkAckV2 {
        fingerprint: crate::ids::FileFingerprint,
    },

    /// A document delete has been enqueued for sink synchronization.
    DeleteDocument { canonical_path: String },

    /// A document delete has been successfully applied to the sink.
    DeleteAck { canonical_path: String },
}

/// Minimal WAL storage contract.
///
/// # Why
/// Runtime code only needs ordered append and replay. Keeping this surface tiny
/// lets us add durable local storage without binding the pipeline to a database.
pub trait WalStore: Send {
    fn append(&mut self, record: WalRecord) -> Result<(), RagloomError>;
    fn read_all(&self) -> Result<Vec<WalRecord>, RagloomError>;
    fn is_empty(&self) -> bool;
}

/// A minimal WAL implementation for unit tests.
///
/// # Why
/// Production WAL will be backed by a persistent store. Unit tests should stay
/// fast and deterministic, so we provide an in-memory implementation.
#[derive(Debug, Default, Clone)]
pub struct InMemoryWal {
    records: Vec<WalRecord>,
}

impl InMemoryWal {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    pub fn append(&mut self, record: WalRecord) -> Result<(), RagloomError> {
        <Self as WalStore>::append(self, record)
    }

    pub fn read_all(&self) -> Result<Vec<WalRecord>, RagloomError> {
        <Self as WalStore>::read_all(self)
    }

    pub fn is_empty(&self) -> bool {
        <Self as WalStore>::is_empty(self)
    }
}

impl WalStore for InMemoryWal {
    fn append(&mut self, record: WalRecord) -> Result<(), RagloomError> {
        self.records.push(record);
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<WalRecord>, RagloomError> {
        Ok(self.records.clone())
    }

    fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Durable newline-delimited JSON WAL.
///
/// # Why
/// A line-oriented JSON format stays inspectable, append-only, and deterministic
/// while being enough to replay work/ack boundaries after a local restart.
/// Ragloom treats the file as durable user-owned state and validates every line
/// on open so unsupported or corrupted records fail before startup replay.
#[derive(Debug)]
pub struct FileWal {
    path: PathBuf,
    file: File,
}

impl FileWal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RagloomError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                    "failed to create WAL directory: {}",
                    parent.display()
                ))
            })?;
        }

        let wal = Self {
            file: open_wal_append_file(&path)?,
            path,
        };
        wal.read_all().map_err(|e| {
            RagloomError::new(e.kind, e).with_context("failed to validate WAL file")
        })?;
        Ok(wal)
    }

    pub fn append(&mut self, record: WalRecord) -> Result<(), RagloomError> {
        <Self as WalStore>::append(self, record)
    }

    pub fn read_all(&self) -> Result<Vec<WalRecord>, RagloomError> {
        <Self as WalStore>::read_all(self)
    }

    pub fn is_empty(&self) -> bool {
        <Self as WalStore>::is_empty(self)
    }
}

impl WalStore for FileWal {
    fn append(&mut self, record: WalRecord) -> Result<(), RagloomError> {
        let encoded = serde_json::to_string(&record).map_err(|e| {
            RagloomError::new(RagloomErrorKind::State, e)
                .with_context("failed to encode WAL record")
        })?;

        // Keep one append handle open to avoid per-record open/close overhead,
        // but still sync every appended record so crash recovery stays explicit.
        writeln!(self.file, "{encoded}").map_err(|e| {
            RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                "failed to append WAL file: {}",
                self.path.display()
            ))
        })?;
        self.file.sync_data().map_err(|e| {
            RagloomError::new(RagloomErrorKind::State, e)
                .with_context(format!("failed to sync WAL file: {}", self.path.display()))
        })?;
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<WalRecord>, RagloomError> {
        let contents = std::fs::read_to_string(&self.path).map_err(|e| {
            RagloomError::new(RagloomErrorKind::State, e)
                .with_context(format!("failed to read WAL file: {}", self.path.display()))
        })?;

        let mut records = Vec::new();
        for (idx, line) in contents.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let record = serde_json::from_str::<WalRecord>(line).map_err(|e| {
                RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                    "failed to parse WAL record at line {} in {}",
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
                    event.name = "ragloom.wal.metadata_failed",
                    path = %self.path.display(),
                    error.message = %err,
                    "ragloom.wal.metadata_failed"
                );
                false
            }
        }
    }
}

fn open_wal_append_file(path: &Path) -> Result<File, RagloomError> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| {
            RagloomError::new(RagloomErrorKind::State, e)
                .with_context(format!("failed to open WAL file: {}", path.display()))
        })
}

/// Returns unacked work in original append order.
pub fn unacked_work_items(records: &[WalRecord]) -> Vec<WalRecord> {
    let mut acked_chunks = std::collections::HashSet::<[u8; 32]>::new();
    let mut acked_files = std::collections::HashSet::<[u8; 32]>::new();
    let mut pending_deletes = std::collections::HashMap::<String, (usize, WalRecord)>::new();

    for (idx, record) in records.iter().enumerate() {
        match record {
            WalRecord::SinkAck { chunk_id } => {
                acked_chunks.insert(*chunk_id);
            }
            WalRecord::SinkAckV2 { fingerprint } => {
                acked_files.insert(crate::ids::file_version_id(fingerprint));
            }
            WalRecord::DeleteDocument { canonical_path } => {
                pending_deletes.insert(canonical_path.clone(), (idx, record.clone()));
            }
            WalRecord::DeleteAck { canonical_path } => {
                pending_deletes.remove(canonical_path);
            }
            WalRecord::WorkItem { .. } | WalRecord::WorkItemV2 { .. } => {}
        }
    }

    let mut unacked = records
        .iter()
        .enumerate()
        .filter_map(|(idx, record)| match record {
            WalRecord::WorkItem { chunk_id } if !acked_chunks.contains(chunk_id) => {
                Some((idx, record.clone()))
            }
            WalRecord::WorkItemV2 { fingerprint }
                if !acked_files.contains(&crate::ids::file_version_id(fingerprint)) =>
            {
                Some((idx, record.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    unacked.extend(pending_deletes.into_values());
    unacked.sort_by_key(|(idx, _)| *idx);
    unacked.into_iter().map(|(_, record)| record).collect()
}

/// Projects the set of canonical paths that were last known to exist.
///
/// # Why
/// The polling source keeps delete detection state in memory. On restart we
/// rebuild the minimum source-side state from the WAL so the next completed
/// scan can emit delete work for files removed while Ragloom was offline.
///
/// Legacy `WorkItem`/`SinkAck` records are intentionally ignored here because
/// they only carry chunk ids, not canonical paths, so they cannot reconstruct
/// source-side document membership for restart-time delete detection.
pub fn known_live_document_paths(records: &[WalRecord]) -> std::collections::HashSet<String> {
    let mut live_paths = std::collections::HashSet::new();

    for record in records {
        match record {
            WalRecord::WorkItemV2 { fingerprint } | WalRecord::SinkAckV2 { fingerprint } => {
                live_paths.insert(fingerprint.canonical_path.clone());
            }
            WalRecord::DeleteDocument { canonical_path }
            | WalRecord::DeleteAck { canonical_path } => {
                live_paths.remove(canonical_path);
            }
            WalRecord::WorkItem { .. } | WalRecord::SinkAck { .. } => {}
        }
    }

    live_paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::FileFingerprint;
    use tempfile::NamedTempFile;

    #[test]
    fn wal_roundtrips_records_in_order() {
        let mut wal = InMemoryWal::new();

        wal.append(WalRecord::WorkItem {
            chunk_id: [1u8; 32],
        })
        .expect("append work item");
        wal.append(WalRecord::SinkAck {
            chunk_id: [1u8; 32],
        })
        .expect("append sink ack");

        let records = wal.read_all().expect("read all");
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0],
            WalRecord::WorkItem {
                chunk_id: [1u8; 32]
            }
        );
        assert_eq!(
            records[1],
            WalRecord::SinkAck {
                chunk_id: [1u8; 32]
            }
        );
    }

    #[test]
    fn wal_reports_empty_state() {
        let wal = InMemoryWal::new();
        assert!(wal.is_empty());
    }

    #[test]
    fn wal_roundtrips_work_item_v2() {
        let mut wal = InMemoryWal::new();

        wal.append(WalRecord::WorkItemV2 {
            fingerprint: crate::ids::FileFingerprint {
                canonical_path: "/x/a.txt".to_string(),
                size_bytes: 10,
                mtime_unix_secs: 100,
                etag: None,
            },
        })
        .expect("append work item v2");

        let records = wal.read_all().expect("read all");
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0],
            WalRecord::WorkItemV2 {
                fingerprint: crate::ids::FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                    etag: None,
                },
            }
        );
    }

    #[test]
    fn file_wal_roundtrips_records_in_order_after_reopen() {
        let file = NamedTempFile::new().expect("temp wal");
        let path = file.path().to_path_buf();

        let mut wal = FileWal::open(&path).expect("open");
        wal.append(WalRecord::WorkItemV2 {
            fingerprint: FileFingerprint {
                canonical_path: "/x/a.txt".to_string(),
                size_bytes: 10,
                mtime_unix_secs: 100,
                etag: None,
            },
        })
        .expect("append work");
        wal.append(WalRecord::SinkAckV2 {
            fingerprint: FileFingerprint {
                canonical_path: "/x/a.txt".to_string(),
                size_bytes: 10,
                mtime_unix_secs: 100,
                etag: None,
            },
        })
        .expect("append ack");

        let reopened = FileWal::open(&path).expect("reopen");
        assert_eq!(
            reopened.read_all().expect("read"),
            vec![
                WalRecord::WorkItemV2 {
                    fingerprint: FileFingerprint {
                        canonical_path: "/x/a.txt".to_string(),
                        size_bytes: 10,
                        mtime_unix_secs: 100,
                        etag: None,
                    },
                },
                WalRecord::SinkAckV2 {
                    fingerprint: FileFingerprint {
                        canonical_path: "/x/a.txt".to_string(),
                        size_bytes: 10,
                        mtime_unix_secs: 100,
                        etag: None,
                    },
                },
            ]
        );
    }

    #[test]
    fn unacked_work_items_filters_acknowledged_file_versions() {
        let a = FileFingerprint {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
            etag: None,
        };
        let b = FileFingerprint {
            canonical_path: "/x/b.txt".to_string(),
            size_bytes: 20,
            mtime_unix_secs: 200,
            etag: None,
        };

        let records = vec![
            WalRecord::WorkItemV2 {
                fingerprint: a.clone(),
            },
            WalRecord::WorkItemV2 {
                fingerprint: b.clone(),
            },
            WalRecord::SinkAckV2 {
                fingerprint: a.clone(),
            },
        ];

        assert_eq!(
            unacked_work_items(&records),
            vec![WalRecord::WorkItemV2 { fingerprint: b }]
        );
    }

    #[test]
    fn unacked_work_items_treats_quoted_and_unquoted_etag_as_same_version() {
        let work = FileFingerprint {
            canonical_path: "s3://docs-bucket/kb/a.md".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
            etag: Some("\"etag-a\"".to_string()),
        };
        let ack = FileFingerprint {
            canonical_path: "s3://docs-bucket/kb/a.md".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
            etag: Some("etag-a".to_string()),
        };

        let records = vec![
            WalRecord::WorkItemV2 {
                fingerprint: work.clone(),
            },
            WalRecord::SinkAckV2 { fingerprint: ack },
        ];

        assert!(unacked_work_items(&records).is_empty());
    }

    #[test]
    fn unacked_work_items_filters_acknowledged_deletes() {
        let records = vec![
            WalRecord::DeleteDocument {
                canonical_path: "/x/a.txt".to_string(),
            },
            WalRecord::DeleteDocument {
                canonical_path: "/x/b.txt".to_string(),
            },
            WalRecord::DeleteAck {
                canonical_path: "/x/a.txt".to_string(),
            },
        ];

        assert_eq!(
            unacked_work_items(&records),
            vec![WalRecord::DeleteDocument {
                canonical_path: "/x/b.txt".to_string()
            }]
        );
    }

    #[test]
    fn unacked_work_items_keeps_later_delete_after_prior_delete_ack() {
        let records = vec![
            WalRecord::DeleteDocument {
                canonical_path: "/x/a.txt".to_string(),
            },
            WalRecord::DeleteAck {
                canonical_path: "/x/a.txt".to_string(),
            },
            WalRecord::DeleteDocument {
                canonical_path: "/x/a.txt".to_string(),
            },
        ];

        assert_eq!(
            unacked_work_items(&records),
            vec![WalRecord::DeleteDocument {
                canonical_path: "/x/a.txt".to_string()
            }]
        );
    }

    #[test]
    fn known_live_document_paths_restores_acknowledged_file_as_live() {
        let records = vec![
            WalRecord::WorkItemV2 {
                fingerprint: FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                    etag: None,
                },
            },
            WalRecord::SinkAckV2 {
                fingerprint: FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                    etag: None,
                },
            },
        ];

        assert_eq!(
            known_live_document_paths(&records),
            std::collections::HashSet::from(["/x/a.txt".to_string()])
        );
    }

    #[test]
    fn known_live_document_paths_removes_path_after_delete() {
        let records = vec![
            WalRecord::WorkItemV2 {
                fingerprint: FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                    etag: None,
                },
            },
            WalRecord::DeleteDocument {
                canonical_path: "/x/a.txt".to_string(),
            },
            WalRecord::DeleteAck {
                canonical_path: "/x/a.txt".to_string(),
            },
        ];

        assert!(known_live_document_paths(&records).is_empty());
    }

    #[test]
    fn known_live_document_paths_restores_path_after_reingest() {
        let records = vec![
            WalRecord::WorkItemV2 {
                fingerprint: FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                    etag: None,
                },
            },
            WalRecord::DeleteAck {
                canonical_path: "/x/a.txt".to_string(),
            },
            WalRecord::WorkItemV2 {
                fingerprint: FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 11,
                    mtime_unix_secs: 101,
                    etag: None,
                },
            },
        ];

        assert_eq!(
            known_live_document_paths(&records),
            std::collections::HashSet::from(["/x/a.txt".to_string()])
        );
    }

    #[test]
    fn known_live_document_paths_keeps_pending_delete_absent() {
        let records = vec![
            WalRecord::WorkItemV2 {
                fingerprint: FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                    etag: None,
                },
            },
            WalRecord::DeleteDocument {
                canonical_path: "/x/a.txt".to_string(),
            },
        ];

        assert!(known_live_document_paths(&records).is_empty());
    }

    #[test]
    fn known_live_document_paths_tracks_s3_locator_without_normalizing_key() {
        let records = vec![WalRecord::WorkItemV2 {
            fingerprint: FileFingerprint {
                canonical_path: "s3://docs-bucket/kb//a.md".to_string(),
                size_bytes: 10,
                mtime_unix_secs: 100,
                etag: Some("\"etag-a\"".to_string()),
            },
        }];

        assert_eq!(
            known_live_document_paths(&records),
            std::collections::HashSet::from(["s3://docs-bucket/kb//a.md".to_string()])
        );
    }

    #[test]
    fn file_wal_reports_invalid_state_with_context() {
        let mut file = NamedTempFile::new().expect("temp wal");
        file.write_all(b"{not json}\n").expect("write");

        let err = FileWal::open(file.path()).expect_err("invalid wal should fail");
        assert_eq!(err.kind, RagloomErrorKind::State);
        assert!(err.to_string().contains("failed to validate WAL file"));
        let source = std::error::Error::source(&err).expect("source");
        assert!(source.to_string().contains("failed to parse WAL record"));
    }

    #[test]
    fn file_wal_is_empty_uses_file_metadata_without_parsing() {
        let file = NamedTempFile::new().expect("temp wal");
        let wal = FileWal::open(file.path()).expect("open");
        assert!(wal.is_empty());

        std::fs::OpenOptions::new()
            .append(true)
            .open(file.path())
            .expect("reopen")
            .write_all(b"{not json}\n")
            .expect("write invalid content");
        assert!(!wal.is_empty());
    }

    #[test]
    fn file_wal_reports_newly_opened_file_as_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let wal = FileWal::open(dir.path().join("fresh.ndjson")).expect("open");

        assert!(wal.is_empty());
    }

    #[test]
    fn file_wal_reuses_open_handle_across_multiple_appends() {
        let file = NamedTempFile::new().expect("temp wal");
        let path = file.path().to_path_buf();

        let mut wal = FileWal::open(&path).expect("open");
        wal.append(WalRecord::WorkItem {
            chunk_id: [1u8; 32],
        })
        .expect("append first");
        wal.append(WalRecord::SinkAck {
            chunk_id: [1u8; 32],
        })
        .expect("append second");

        assert_eq!(
            wal.read_all().expect("read"),
            vec![
                WalRecord::WorkItem {
                    chunk_id: [1u8; 32]
                },
                WalRecord::SinkAck {
                    chunk_id: [1u8; 32]
                },
            ]
        );
    }
}
