//! Polling directory scanner source.
//!
//! # Why
//! Some environments (or MVP phases) do not provide reliable filesystem event
//! notifications. A polling scanner keeps the ingestion pipeline functional by
//! periodically enumerating a directory and translating file metadata into
//! stable file-version discovery events.

use std::path::{Path, PathBuf};

use crate::source::file_tailer::{FileTailer, ObservedFileMeta};
use crate::source::{FileVersionDiscovered, Source};

/// A polling source that scans a root directory (one level) for files.
///
/// # Why
/// We keep scanning concerns separate from change detection: this type only
/// enumerates filesystem entries and delegates change/version logic to
/// [`FileTailer`].
#[derive(Debug)]
pub struct DirectoryScannerSource {
    root: PathBuf,
    tailer: FileTailer,
}

impl DirectoryScannerSource {
    /// Creates a new scanner rooted at `root`.
    ///
    /// # Why
    /// The scanner is stateful (it must remember previously observed versions)
    /// so construction is explicit and fallible only for invalid inputs.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        Ok(Self {
            root: root.as_ref().to_path_buf(),
            tailer: FileTailer::new(),
        })
    }

    fn observe_root_once(&mut self) {
        let Ok(iter) = std::fs::read_dir(&self.root) else {
            return;
        };

        for entry in iter.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }

            let path = entry.path();
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };

            let size_bytes = meta.len();
            let mtime_unix_secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            self.tailer.observe(ObservedFileMeta {
                canonical_path: path.to_string_lossy().to_string(),
                size_bytes,
                mtime_unix_secs,
            });
        }
    }
}

impl Source for DirectoryScannerSource {
    fn poll(&mut self) -> Vec<FileVersionDiscovered> {
        self.observe_root_once();
        self.tailer.drain()
    }
}
