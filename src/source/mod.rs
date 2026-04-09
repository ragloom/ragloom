//! Source abstractions.
//!
//! # Why
//! Sources are responsible for discovering new inputs and reporting stable
//! identities for incremental processing. They should not embed downstream
//! processing concerns (chunking/embedding/sink) to keep the system open for
//! extension.

pub mod dir_scanner;
pub mod file_tailer;

pub use dir_scanner::DirectoryScannerSource;

use crate::ids::FileFingerprint;

/// A source event that describes a discovered file version.
///
/// # Why
/// We intentionally emit file-version granularity events so downstream stages
/// can decide how to expand work (chunking strategy, batching, etc.) without
/// modifying the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileVersionDiscovered {
    pub fingerprint: FileFingerprint,
    pub file_version_id: [u8; 32],
}

/// A generic source of file-version discovery events.
///
/// # Why
/// This trait keeps the pipeline open for new sources (S3/Notion/etc.) while
/// enabling deterministic tests by providing a simple pull-based interface.
pub trait Source {
    fn poll(&mut self) -> Vec<FileVersionDiscovered>;
}
