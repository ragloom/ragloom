//! Polling directory scanner source.
//!
//! # Why
//! Some environments (or MVP phases) do not provide reliable filesystem event
//! notifications. A polling scanner keeps the ingestion pipeline functional by
//! periodically enumerating a directory tree and translating file metadata
//! into stable file-version discovery events.

use std::path::{Path, PathBuf};

use crate::source::file_tailer::{FileTailer, ObservedFileMeta};
use crate::source::{FileVersionDiscovered, Source};

/// A polling source that scans a root directory tree for files.
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
        for path in walk_regular_files(&self.root) {
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

fn walk_regular_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(dir) = pending.pop() {
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };

        let mut entries = read_dir.flatten().collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if file_type.is_file() {
                files.push(path);
                continue;
            }

            if file_type.is_dir() {
                pending.push(path);
            }
        }
    }

    files
}

impl Source for DirectoryScannerSource {
    fn poll(&mut self) -> Vec<FileVersionDiscovered> {
        self.observe_root_once();
        self.tailer.drain()
    }
}
