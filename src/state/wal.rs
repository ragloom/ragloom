//! Write-ahead log (WAL) record types.
//!
//! # Why
//! We rely on at-least-once execution with deterministic IDs. A minimal WAL lets
//! us replay un-acked work after crashes without requiring complex distributed
//! coordination.

use crate::error::RagloomError;

/// A durable record representing progress at the pipeline boundaries.
///
/// # Why
/// Only boundary events (enqueue work, acknowledge sink write) need to be
/// persisted to enable replay.
#[derive(Debug, Clone, PartialEq, Eq)]
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
        self.records.push(record);
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<WalRecord>, RagloomError> {
        Ok(self.records.clone())
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
