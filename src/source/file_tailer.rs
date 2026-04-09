//! Local filesystem discovery and change detection.
//!
//! # Why
//! The daemon needs a reliable way to discover new/changed files without
//! coupling discovery to downstream processing. The file tailer emits
//! file-version events using the MVP fingerprint strategy.

use std::collections::HashMap;

use crate::ids::{FileFingerprint, file_version_id};
use crate::source::FileVersionDiscovered;

/// Internal representation of file metadata used for deterministic tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedFileMeta {
    pub canonical_path: String,
    pub size_bytes: u64,
    pub mtime_unix_secs: i64,
}

impl ObservedFileMeta {
    fn to_fingerprint(&self) -> FileFingerprint {
        FileFingerprint {
            canonical_path: self.canonical_path.clone(),
            size_bytes: self.size_bytes,
            mtime_unix_secs: self.mtime_unix_secs,
        }
    }
}

/// A minimal file tailer state machine.
///
/// # Why
/// For TDD, we separate the pure state machine (tested via injected observations)
/// from any real filesystem scanning/watching implementation.
#[derive(Debug, Default)]
pub struct FileTailer {
    last_seen_version: HashMap<String, [u8; 32]>,
    pending: Vec<FileVersionDiscovered>,
}

impl FileTailer {
    /// Constructs a new tailer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds an observation into the tailer.
    ///
    /// # Why
    /// This method enables deterministic unit tests without touching the OS.
    pub fn observe(&mut self, meta: ObservedFileMeta) {
        let fingerprint = meta.to_fingerprint();
        let version_id = file_version_id(&fingerprint);

        let should_emit = self
            .last_seen_version
            .get(&fingerprint.canonical_path)
            .map(|existing| existing != &version_id)
            .unwrap_or(true);

        if should_emit {
            self.last_seen_version
                .insert(fingerprint.canonical_path.clone(), version_id);
            self.pending.push(FileVersionDiscovered {
                fingerprint,
                file_version_id: version_id,
            });
        }
    }

    /// Drains pending discovery events.
    pub fn drain(&mut self) -> Vec<FileVersionDiscovered> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_fingerprint_does_not_emit_new_version_event() {
        let mut tailer = FileTailer::new();

        tailer.observe(ObservedFileMeta {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
        });
        assert_eq!(tailer.drain().len(), 1);

        tailer.observe(ObservedFileMeta {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
        });
        assert_eq!(tailer.drain().len(), 0);
    }

    #[test]
    fn changed_mtime_emits_new_version_event() {
        let mut tailer = FileTailer::new();

        tailer.observe(ObservedFileMeta {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 100,
        });
        let first = tailer.drain();
        assert_eq!(first.len(), 1);

        tailer.observe(ObservedFileMeta {
            canonical_path: "/x/a.txt".to_string(),
            size_bytes: 10,
            mtime_unix_secs: 101,
        });
        let second = tailer.drain();
        assert_eq!(second.len(), 1);
        assert_ne!(first[0].file_version_id, second[0].file_version_id);
    }
}
