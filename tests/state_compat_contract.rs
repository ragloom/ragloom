use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use ragloom::RagloomErrorKind;
use ragloom::ids::FileFingerprint;
use ragloom::startup::{
    CompactStateConfig, ReplayFailedConfig, compact_state_command, replay_failed_command,
};
use ragloom::state::failed::{
    FailedWorkRecord, FileFailedWorkStore, next_failed_work_id, pending_failed_work,
};
use ragloom::state::wal::{FileWal, WalRecord, known_live_document_paths, unacked_work_items};

fn fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("state")
        .join(relative)
}

fn copy_fixture_tree(relative: &str) -> tempfile::TempDir {
    let src = fixture_path(relative);
    let dst = tempfile::tempdir().expect("temp dir");
    copy_dir_recursive(&src, dst.path());
    dst
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create temp fixture dir");
    for entry in fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("fixture entry");
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type().expect("entry type").is_dir() {
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            fs::copy(&src_path, &dst_path).expect("copy fixture file");
        }
    }
}

fn fp(path: &str, size_bytes: u64, mtime_unix_secs: i64) -> FileFingerprint {
    FileFingerprint {
        canonical_path: path.to_string(),
        size_bytes,
        mtime_unix_secs,
        etag: None,
    }
}

fn fp_with_etag(path: &str, size_bytes: u64, mtime_unix_secs: i64, etag: &str) -> FileFingerprint {
    FileFingerprint {
        etag: Some(etag.to_string()),
        ..fp(path, size_bytes, mtime_unix_secs)
    }
}

#[test]
fn v0_4_0_wal_fixture_is_directly_readable() {
    let wal = FileWal::open(fixture_path("v0.4.0/wal.ndjson")).expect("open v0.4.0 wal");
    let records = wal.read_all().expect("read v0.4.0 wal");

    assert_eq!(
        unacked_work_items(&records),
        vec![
            WalRecord::WorkItemV2 {
                fingerprint: fp("/docs/pending.md", 120, 1_710_000_100),
            },
            WalRecord::WorkItem {
                chunk_id: [2u8; 32],
            },
            WalRecord::DeleteDocument {
                canonical_path: "/docs/pending-delete.md".to_string(),
            },
        ]
    );
    assert_eq!(
        known_live_document_paths(&records),
        HashSet::from(["/docs/acked.md".to_string(), "/docs/pending.md".to_string(),])
    );
}

#[test]
fn v0_4_1_state_fixtures_are_directly_readable() {
    let wal = FileWal::open(fixture_path("v0.4.1/wal.ndjson")).expect("open v0.4.1 wal");
    let failed =
        FileFailedWorkStore::open(fixture_path("v0.4.1/failed.ndjson")).expect("open failed");
    let wal_records = wal.read_all().expect("read v0.4.1 wal");
    let failed_records = failed.read_all().expect("read v0.4.1 failed");

    assert_eq!(
        unacked_work_items(&wal_records),
        vec![
            WalRecord::WorkItemV2 {
                fingerprint: fp("/docs/pending.md", 140, 1_710_000_200),
            },
            WalRecord::DeleteDocument {
                canonical_path: "/docs/pending-delete.md".to_string(),
            },
            WalRecord::WorkItem {
                chunk_id: [17u8; 32],
            },
        ]
    );
    assert_eq!(
        known_live_document_paths(&wal_records),
        HashSet::from(["/docs/acked.md".to_string(), "/docs/pending.md".to_string(),])
    );
    assert_eq!(
        pending_failed_work(&failed_records)
            .into_iter()
            .map(|record| record.id)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(next_failed_work_id(&failed_records), 4);
}

#[test]
fn v0_5_0_state_fixtures_are_directly_readable() {
    let wal = FileWal::open(fixture_path("v0.5.0/wal.ndjson")).expect("open v0.5.0 wal");
    let failed =
        FileFailedWorkStore::open(fixture_path("v0.5.0/failed.ndjson")).expect("open failed");
    let wal_records = wal.read_all().expect("read v0.5.0 wal");
    let failed_records = failed.read_all().expect("read v0.5.0 failed");

    assert_eq!(
        unacked_work_items(&wal_records),
        vec![
            WalRecord::WorkItemV2 {
                fingerprint: fp_with_etag("/docs/v0.5-pending.md", 220, 1_710_000_600, "v0.5-etag",),
            },
            WalRecord::DeleteDocument {
                canonical_path: "/docs/v0.5-pending-delete.md".to_string(),
            },
        ]
    );
    assert_eq!(
        known_live_document_paths(&wal_records),
        HashSet::from([
            "/docs/v0.5-acked.md".to_string(),
            "/docs/v0.5-pending.md".to_string(),
        ])
    );
    assert_eq!(
        pending_failed_work(&failed_records)
            .into_iter()
            .map(|record| record.id)
            .collect::<Vec<_>>(),
        vec![2]
    );
    assert_eq!(next_failed_work_id(&failed_records), 3);
}

#[tokio::test]
async fn replay_failed_command_preserves_v0_4_0_empty_failed_surface() {
    let dir = copy_fixture_tree("v0.4.0");
    let wal_path = dir.path().join("wal.ndjson");
    let failed_path = dir.path().join("failed.ndjson");

    let replayed = replay_failed_command(&ReplayFailedConfig {
        state_path: wal_path.to_string_lossy().to_string(),
    })
    .await
    .expect("replay v0.4.0");

    assert_eq!(replayed.pending, 0, "empty failed-work surface");
    assert_eq!(replayed.requeued, 0, "no items to requeue");
    assert_eq!(replayed.skipped, 0, "no already-requeued items");
    assert_eq!(replayed.failed, 0, "replay succeeded");
    assert!(failed_path.exists());
    assert_eq!(fs::read_to_string(&failed_path).expect("read failed"), "");
}

#[tokio::test]
async fn replay_failed_command_replays_pending_v0_4_1_failed_work() {
    let dir = copy_fixture_tree("v0.4.1");
    let wal_path = dir.path().join("wal.ndjson");
    let failed_path = dir.path().join("failed.ndjson");

    let replayed = replay_failed_command(&ReplayFailedConfig {
        state_path: wal_path.to_string_lossy().to_string(),
    })
    .await
    .expect("replay v0.4.1");

    assert_eq!(replayed.pending, 2, "v0.4.1 fixture has 2 pending items");
    assert_eq!(replayed.requeued, 2, "both pending items were requeued");
    assert_eq!(replayed.skipped, 1, "one prior item was already requeued");
    assert_eq!(replayed.failed, 0, "replay succeeded");

    let wal_records = FileWal::open(&wal_path)
        .expect("reopen wal")
        .read_all()
        .expect("read wal after replay");
    assert_eq!(
        &wal_records[wal_records.len() - 2..],
        &[
            WalRecord::DeleteDocument {
                canonical_path: "/docs/retry-delete.md".to_string(),
            },
            WalRecord::WorkItemV2 {
                fingerprint: fp("/docs/retry-file.md", 160, 1_710_000_400),
            },
        ]
    );

    let failed_records = FileFailedWorkStore::open(&failed_path)
        .expect("reopen failed")
        .read_all()
        .expect("read failed after replay");
    assert!(pending_failed_work(&failed_records).is_empty());
    assert_eq!(next_failed_work_id(&failed_records), 4);
    assert_eq!(
        failed_records[failed_records.len() - 2..],
        [
            FailedWorkRecord::Requeued { exhausted_id: 2 },
            FailedWorkRecord::Requeued { exhausted_id: 3 },
        ]
    );
}

#[tokio::test]
async fn replay_failed_command_replays_pending_v0_5_0_failed_work() {
    let dir = copy_fixture_tree("v0.5.0");
    let wal_path = dir.path().join("wal.ndjson");
    let failed_path = dir.path().join("failed.ndjson");

    let replayed = replay_failed_command(&ReplayFailedConfig {
        state_path: wal_path.to_string_lossy().to_string(),
    })
    .await
    .expect("replay v0.5.0");

    assert_eq!(replayed.pending, 1);
    assert_eq!(replayed.requeued, 1);
    assert_eq!(replayed.skipped, 1);
    assert_eq!(replayed.failed, 0);

    let wal_records = FileWal::open(&wal_path)
        .expect("reopen wal")
        .read_all()
        .expect("read wal after replay");
    assert_eq!(
        wal_records.last(),
        Some(&WalRecord::WorkItemV2 {
            fingerprint: fp_with_etag("/docs/v0.5-retry.md", 240, 1_710_000_800, "retry-etag",),
        })
    );

    let failed_records = FileFailedWorkStore::open(&failed_path)
        .expect("reopen failed")
        .read_all()
        .expect("read failed after replay");
    assert!(pending_failed_work(&failed_records).is_empty());
    assert_eq!(next_failed_work_id(&failed_records), 3);
}

#[tokio::test]
async fn compact_state_command_preserves_v0_4_0_observables() {
    let dir = copy_fixture_tree("v0.4.0");
    let wal_path = dir.path().join("wal.ndjson");

    let before = FileWal::open(&wal_path)
        .expect("open wal before")
        .read_all()
        .expect("read wal before");

    let summary = compact_state_command(&CompactStateConfig {
        state_path: wal_path.to_string_lossy().to_string(),
    })
    .await
    .expect("compact v0.4.0");

    assert!(summary.wal.records_after <= summary.wal.records_before);

    let after = FileWal::open(&wal_path)
        .expect("open wal after")
        .read_all()
        .expect("read wal after");

    assert_eq!(unacked_work_items(&after), unacked_work_items(&before));
    assert_eq!(
        known_live_document_paths(&after),
        known_live_document_paths(&before)
    );
}

#[tokio::test]
async fn compact_state_command_preserves_v0_4_1_observables() {
    let dir = copy_fixture_tree("v0.4.1");
    let wal_path = dir.path().join("wal.ndjson");
    let failed_path = dir.path().join("failed.ndjson");

    let wal_before = FileWal::open(&wal_path)
        .expect("open wal before")
        .read_all()
        .expect("read wal before");
    let failed_before = FileFailedWorkStore::open(&failed_path)
        .expect("open failed before")
        .read_all()
        .expect("read failed before");

    let summary = compact_state_command(&CompactStateConfig {
        state_path: wal_path.to_string_lossy().to_string(),
    })
    .await
    .expect("compact v0.4.1");

    assert!(summary.wal.records_after <= summary.wal.records_before);
    assert!(summary.failed_work.records_after <= summary.failed_work.records_before);

    let wal_after = FileWal::open(&wal_path)
        .expect("open wal after")
        .read_all()
        .expect("read wal after");
    let failed_after = FileFailedWorkStore::open(&failed_path)
        .expect("open failed after")
        .read_all()
        .expect("read failed after");

    assert_eq!(
        unacked_work_items(&wal_after),
        unacked_work_items(&wal_before)
    );
    assert_eq!(
        known_live_document_paths(&wal_after),
        known_live_document_paths(&wal_before)
    );
    assert_eq!(
        pending_failed_work(&failed_after),
        pending_failed_work(&failed_before)
    );
    assert_eq!(
        next_failed_work_id(&failed_after),
        next_failed_work_id(&failed_before)
    );
}

#[tokio::test]
async fn compact_state_command_preserves_v0_5_0_observables() {
    let dir = copy_fixture_tree("v0.5.0");
    let wal_path = dir.path().join("wal.ndjson");
    let failed_path = dir.path().join("failed.ndjson");

    let wal_before = FileWal::open(&wal_path)
        .expect("open wal before")
        .read_all()
        .expect("read wal before");
    let failed_before = FileFailedWorkStore::open(&failed_path)
        .expect("open failed before")
        .read_all()
        .expect("read failed before");

    let summary = compact_state_command(&CompactStateConfig {
        state_path: wal_path.to_string_lossy().to_string(),
    })
    .await
    .expect("compact v0.5.0");

    assert!(summary.wal.records_after <= summary.wal.records_before);
    assert!(summary.failed_work.records_after <= summary.failed_work.records_before);

    let wal_after = FileWal::open(&wal_path)
        .expect("open wal after")
        .read_all()
        .expect("read wal after");
    let failed_after = FileFailedWorkStore::open(&failed_path)
        .expect("open failed after")
        .read_all()
        .expect("read failed after");

    assert_eq!(
        unacked_work_items(&wal_after),
        unacked_work_items(&wal_before)
    );
    assert_eq!(
        known_live_document_paths(&wal_after),
        known_live_document_paths(&wal_before)
    );
    assert_eq!(
        pending_failed_work(&failed_after),
        pending_failed_work(&failed_before)
    );
    assert_eq!(
        next_failed_work_id(&failed_after),
        next_failed_work_id(&failed_before)
    );
}

#[test]
fn unknown_future_wal_records_fail_closed_with_context() {
    let err = FileWal::open(fixture_path("invalid/unknown-wal-record.ndjson"))
        .expect_err("future wal record should fail");

    assert_eq!(err.kind, RagloomErrorKind::State);
    assert!(err.to_string().contains("failed to validate WAL file"));
    let source = std::error::Error::source(&err).expect("source");
    assert!(
        source
            .to_string()
            .contains("failed to parse WAL record at line 1")
    );
}

#[test]
fn malformed_wal_records_fail_closed_with_context() {
    let err = FileWal::open(fixture_path("invalid/malformed-wal.ndjson"))
        .expect_err("malformed wal should fail");

    assert_eq!(err.kind, RagloomErrorKind::State);
    assert!(err.to_string().contains("failed to validate WAL file"));
    let source = std::error::Error::source(&err).expect("source");
    assert!(
        source
            .to_string()
            .contains("failed to parse WAL record at line 1")
    );
}

#[test]
fn truncated_final_wal_record_fails_closed_with_context() {
    let err = FileWal::open(fixture_path("invalid/truncated-wal.ndjson"))
        .expect_err("truncated wal should fail");

    assert_eq!(err.kind, RagloomErrorKind::State);
    assert!(err.to_string().contains("failed to validate WAL file"));
    let source = std::error::Error::source(&err).expect("source");
    assert!(
        source
            .to_string()
            .contains("failed to parse WAL record at line 2")
    );
}

#[test]
fn unknown_future_failed_work_records_fail_closed_with_context() {
    let err = FileFailedWorkStore::open(fixture_path("invalid/unknown-failed.ndjson"))
        .expect_err("future failed-work record should fail");

    assert_eq!(err.kind, RagloomErrorKind::State);
    assert!(
        err.to_string()
            .contains("failed to validate failed-work file")
    );
    let source = std::error::Error::source(&err).expect("source");
    assert!(
        source
            .to_string()
            .contains("failed to parse failed-work record at line 1")
    );
}
