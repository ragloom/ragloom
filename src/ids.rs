//! Deterministic identifiers used across the ingestion pipeline.
//!
//! # Why
//! Ragloom aims for near exactly-once effects by making sink writes idempotent.
//! We achieve this by deriving stable identifiers from stable inputs. MVP uses
//! a cheap file fingerprint (path + size + mtime) to avoid content hashing.

use blake3::Hasher;

/// A cheap file fingerprint used for incremental ingestion.
///
/// # Why
/// The MVP prioritizes throughput over perfect change detection.
/// A (path, size, mtime) fingerprint is fast and good enough for many workloads.
///
/// This type is intentionally small and explicit so it can be constructed from
/// multiple sources (filesystem scan, file watcher events, tests).
#[derive(Debug, Clone, PartialEq, Eq, std::hash::Hash, serde::Serialize, serde::Deserialize)]
pub struct FileFingerprint {
    pub canonical_path: String,
    pub size_bytes: u64,
    pub mtime_unix_secs: i64,
}

/// Computes a stable 32-byte identifier for a file version.
///
/// # Why
/// Downstream identifiers (chunk_id, point_id) should change only when the file
/// changes in a way we care about for the MVP. Using a stable hash keeps IDs
/// deterministic across restarts.
pub fn file_version_id(fingerprint: &FileFingerprint) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(fingerprint.canonical_path.as_bytes());
    hasher.update(&fingerprint.size_bytes.to_le_bytes());
    hasher.update(&fingerprint.mtime_unix_secs.to_le_bytes());
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_version_id_is_stable_for_identical_fingerprint() {
        let fingerprint = FileFingerprint {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
        };
        assert_eq!(file_version_id(&fingerprint), file_version_id(&fingerprint));
    }

    #[test]
    fn file_version_id_changes_when_size_changes() {
        let a = FileFingerprint {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
        };
        let b = FileFingerprint {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 11,
            mtime_unix_secs: 100,
        };
        assert_ne!(file_version_id(&a), file_version_id(&b));
    }

    #[test]
    fn file_version_id_changes_when_mtime_changes() {
        let a = FileFingerprint {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
        };
        let b = FileFingerprint {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 101,
        };
        assert_ne!(file_version_id(&a), file_version_id(&b));
    }

    #[test]
    fn file_version_id_changes_when_path_changes() {
        let a = FileFingerprint {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
        };
        let b = FileFingerprint {
            canonical_path: "/x/b.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
        };
        assert_ne!(file_version_id(&a), file_version_id(&b));
    }
}
