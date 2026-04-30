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
        walk_regular_files(&self.root, |path| {
            let meta = match std::fs::metadata(&path) {
                Ok(meta) => meta,
                Err(error) => {
                    tracing::trace!(
                        event.name = "ragloom.source.dir_scanner.skip_metadata",
                        path = %path.display(),
                        error = %error,
                        "ragloom.source.dir_scanner.skip_metadata"
                    );
                    return;
                }
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
        });
    }
}

fn walk_regular_files(root: &Path, mut visit: impl FnMut(PathBuf)) {
    walk_regular_files_inner(root, &mut visit);
}

fn walk_regular_files_inner(root: &Path, visit: &mut dyn FnMut(PathBuf)) {
    let read_dir = match std::fs::read_dir(root) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            tracing::trace!(
                event.name = "ragloom.source.dir_scanner.skip_dir",
                path = %root.display(),
                error = %error,
                "ragloom.source.dir_scanner.skip_dir"
            );
            return;
        }
    };

    let mut entries = Vec::new();
    for entry in read_dir {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(error) => {
                tracing::trace!(
                    event.name = "ragloom.source.dir_scanner.skip_entry",
                    path = %root.display(),
                    error = %error,
                    "ragloom.source.dir_scanner.skip_entry"
                );
            }
        }
    }
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                tracing::trace!(
                    event.name = "ragloom.source.dir_scanner.skip_file_type",
                    path = %path.display(),
                    error = %error,
                    "ragloom.source.dir_scanner.skip_file_type"
                );
                continue;
            }
        };

        if file_type.is_file() {
            visit(path);
            continue;
        }

        if file_type.is_dir() {
            walk_regular_files_inner(&path, visit);
        }
    }
}

impl Source for DirectoryScannerSource {
    fn poll(&mut self) -> Vec<FileVersionDiscovered> {
        self.observe_root_once();
        self.tailer.drain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    use tempfile::tempdir;

    #[test]
    fn walk_regular_files_visits_nested_files_in_sorted_path_order() {
        let tmp = tempdir().expect("create tempdir");
        let a_dir = tmp.path().join("a");
        let b_dir = tmp.path().join("b");
        fs::create_dir_all(&a_dir).expect("create a dir");
        fs::create_dir_all(&b_dir).expect("create b dir");

        write_text_file(&a_dir.join("one.txt"), "a");
        write_text_file(&b_dir.join("one.txt"), "b");

        let mut paths = Vec::new();
        walk_regular_files(tmp.path(), |path| paths.push(path));
        let names: Vec<String> = paths
            .into_iter()
            .map(|path| {
                path.strip_prefix(tmp.path())
                    .expect("path under tempdir")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();

        assert_eq!(names, vec!["a/one.txt", "b/one.txt"]);
    }

    #[test]
    #[tracing_test::traced_test]
    fn missing_root_logs_trace_when_directory_cannot_be_read() {
        let missing_root = PathBuf::from("definitely-missing-ragloom-dir");

        let mut scanner = DirectoryScannerSource::new(&missing_root).expect("create scanner");

        let events = scanner.poll();
        assert!(events.is_empty());
        assert!(
            logs_contain("ragloom.source.dir_scanner.skip_dir"),
            "expected ragloom.source.dir_scanner.skip_dir trace event"
        );
    }

    fn write_text_file(path: &Path, contents: &str) {
        let mut file = fs::File::create(path).expect("create file");
        write!(file, "{contents}").expect("write file");
    }
}
