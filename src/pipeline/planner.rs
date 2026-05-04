//! Work planning: expand discovery events into durable work items.
//!
//! # Why
//! Sources should report *what changed* (a new file version). The pipeline then
//! decides *how to process it* (chunking strategy, batching, etc.). This keeps
//! discovery open for extension and keeps processing strategies replaceable.

use std::collections::HashSet;

use crate::source::FileVersionDiscovered;
use crate::state::wal::{WalRecord, WalStore};

use crate::error::RagloomError;

/// Plans work items for newly discovered file versions.
///
/// # Why
/// This planner is the first place we can enforce idempotency rules:
/// the same file version should not repeatedly enqueue identical work.
#[derive(Debug, Default)]
pub struct Planner {
    planned_versions: HashSet<[u8; 32]>,
}

impl Planner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_wal_records(records: &[WalRecord]) -> Self {
        let planned_versions = records
            .iter()
            .map(|record| match record {
                WalRecord::WorkItemV2 { fingerprint } | WalRecord::SinkAckV2 { fingerprint } => {
                    crate::ids::file_version_id(fingerprint)
                }
                WalRecord::WorkItem { chunk_id } | WalRecord::SinkAck { chunk_id } => *chunk_id,
            })
            .collect();
        Self { planned_versions }
    }

    /// Plans work for a discovery event.
    ///
    /// # Why
    /// This method is deterministic and side-effect free except for writing to
    /// the provided WAL, making it easy to unit test.
    pub fn plan_file_version(
        &mut self,
        discovered: &FileVersionDiscovered,
        wal: &std::sync::Arc<tokio::sync::Mutex<impl WalStore>>,
    ) -> Result<(), RagloomError> {
        if !self.planned_versions.insert(discovered.file_version_id) {
            return Ok(());
        }

        // MVP (V2): plan work using the on-disk file fingerprint rather than a
        // precomputed chunk ID. This enables deterministic ID reconstruction
        // after restarts without requiring chunking logic at the planner stage.
        let mut guard = wal.try_lock().map_err(|_| {
            RagloomError::from_kind(crate::error::RagloomErrorKind::Internal)
                .with_context("wal is currently locked")
        })?;
        guard.append(WalRecord::WorkItemV2 {
            fingerprint: discovered.fingerprint.clone(),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::FileFingerprint;

    fn discovered(version_id: [u8; 32]) -> FileVersionDiscovered {
        FileVersionDiscovered {
            fingerprint: FileFingerprint {
                canonical_path: "/x/a.txt".to_string(),
                size_bytes: 10,
                mtime_unix_secs: 100,
            },
            file_version_id: version_id,
        }
    }

    #[test]
    fn planner_emits_work_item_v2_with_fingerprint() {
        let mut planner = Planner::new();
        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));

        let v = [7u8; 32];
        planner
            .plan_file_version(&discovered(v), &wal)
            .expect("plan");

        let records = wal.try_lock().expect("wal").read_all().expect("read all");
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
    fn planner_does_not_enqueue_duplicate_work_for_same_file_version() {
        let mut planner = Planner::new();
        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));

        let v = [7u8; 32];
        planner
            .plan_file_version(&discovered(v), &wal)
            .expect("plan");
        planner
            .plan_file_version(&discovered(v), &wal)
            .expect("plan");

        let records = wal.try_lock().expect("wal").read_all().expect("read all");
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

    #[tokio::test]
    async fn planner_returns_error_when_wal_is_locked() {
        let mut planner = Planner::new();
        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));

        let _guard = wal.lock().await;

        let err = planner
            .plan_file_version(&discovered([1u8; 32]), &wal)
            .expect_err("should fail when wal is locked");
        assert_eq!(err.kind, crate::error::RagloomErrorKind::Internal);
        assert!(err.to_string().contains("wal is currently locked"));
    }
}
