//! Document loading abstractions.

use async_trait::async_trait;

use crate::{RagloomError, RagloomErrorKind};

/// Loads document bytes/text from a backing store.
///
/// # Why
/// Ingestion pipelines should depend on a small abstraction rather than hard-coding
/// filesystem, HTTP, or object-store logic. This trait provides a stable surface
/// for the pipeline while enabling alternate loaders in tests or other runtimes.
#[async_trait]
pub trait DocumentLoader: Send + Sync {
    /// Loads a UTF-8 document and returns its text.
    async fn load_utf8(&self, path: &str) -> Result<String, RagloomError>;
}

/// Loads UTF-8 documents from the local filesystem.
///
/// # Why
/// The MVP ingest path often starts from local files. This loader keeps the
/// filesystem concerns encapsulated and returns crate-level errors with context.
#[derive(Debug, Default, Clone, Copy)]
pub struct FsUtf8Loader;

#[async_trait]
impl DocumentLoader for FsUtf8Loader {
    async fn load_utf8(&self, path: &str) -> Result<String, RagloomError> {
        let bytes = tokio::fs::read(path).await.map_err(|e| {
            RagloomError::new(RagloomErrorKind::Io, e).with_context("failed to read utf-8 file")
        })?;

        let text = String::from_utf8(bytes).map_err(|e| {
            RagloomError::new(RagloomErrorKind::InvalidInput, e)
                .with_context("failed to read utf-8 file")
        })?;

        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fs_utf8_loader_reads_text_file() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("hello.txt");
        tokio::fs::write(&path, "hello\nworld")
            .await
            .expect("write file");

        let loader = FsUtf8Loader;
        let loaded = loader
            .load_utf8(path.to_str().expect("utf-8 path"))
            .await
            .expect("load file");

        assert_eq!(loaded, "hello\nworld");
    }

    #[tokio::test]
    async fn fs_utf8_loader_returns_error_with_context_on_missing_file() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("missing.txt");

        let loader = FsUtf8Loader;
        let err = loader
            .load_utf8(path.to_str().expect("utf-8 path"))
            .await
            .expect_err("expected error");

        assert_eq!(err.kind, RagloomErrorKind::Io);
        assert!(err.to_string().contains("failed to read utf-8 file"));
    }
}
