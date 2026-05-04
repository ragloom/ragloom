//! Write-ahead log (WAL) record types.
//!
//! # Why
//! We rely on at-least-once execution with deterministic IDs. A minimal WAL lets
//! us replay un-acked work after crashes without requiring complex distributed
//! coordination.

use std::io::Write;
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
#[derive(Debug)]
pub struct FileWal {
    path: PathBuf,
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

        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::State, e)
                    .with_context(format!("failed to open WAL file: {}", path.display()))
            })?;

        let wal = Self { path };
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

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| {
                RagloomError::new(RagloomErrorKind::State, e)
                    .with_context(format!("failed to open WAL file: {}", self.path.display()))
            })?;
        writeln!(file, "{encoded}").map_err(|e| {
            RagloomError::new(RagloomErrorKind::State, e).with_context(format!(
                "failed to append WAL file: {}",
                self.path.display()
            ))
        })?;
        file.sync_data().map_err(|e| {
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
        self.read_all()
            .map(|records| records.is_empty())
            .unwrap_or(false)
    }
}

/// Returns unacked work in original append order.
pub fn unacked_work_items(records: &[WalRecord]) -> Vec<WalRecord> {
    let mut acked_chunks = std::collections::HashSet::<[u8; 32]>::new();
    let mut acked_files = std::collections::HashSet::<crate::ids::FileFingerprint>::new();

    for record in records {
        match record {
            WalRecord::SinkAck { chunk_id } => {
                acked_chunks.insert(*chunk_id);
            }
            WalRecord::SinkAckV2 { fingerprint } => {
                acked_files.insert(fingerprint.clone());
            }
            WalRecord::WorkItem { .. } | WalRecord::WorkItemV2 { .. } => {}
        }
    }

    records
        .iter()
        .filter_map(|record| match record {
            WalRecord::WorkItem { chunk_id } if !acked_chunks.contains(chunk_id) => {
                Some(record.clone())
            }
            WalRecord::WorkItemV2 { fingerprint } if !acked_files.contains(fingerprint) => {
                Some(record.clone())
            }
            _ => None,
        })
        .collect()
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
            },
        })
        .expect("append work");
        wal.append(WalRecord::SinkAckV2 {
            fingerprint: FileFingerprint {
                canonical_path: "/x/a.txt".to_string(),
                size_bytes: 10,
                mtime_unix_secs: 100,
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
                    },
                },
                WalRecord::SinkAckV2 {
                    fingerprint: FileFingerprint {
                        canonical_path: "/x/a.txt".to_string(),
                        size_bytes: 10,
                        mtime_unix_secs: 100,
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
        };
        let b = FileFingerprint {
            canonical_path: "/x/b.txt".to_string(),
            size_bytes: 20,
            mtime_unix_secs: 200,
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
    fn file_wal_reports_invalid_state_with_context() {
        let mut file = NamedTempFile::new().expect("temp wal");
        file.write_all(b"{not json}\n").expect("write");

        let err = FileWal::open(file.path()).expect_err("invalid wal should fail");
        assert_eq!(err.kind, RagloomErrorKind::State);
        assert!(err.to_string().contains("failed to validate WAL file"));
        let source = std::error::Error::source(&err).expect("source");
        assert!(source.to_string().contains("failed to parse WAL record"));
    }
}
