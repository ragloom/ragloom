//! Sync signal-source trait for semantic chunking.
//!
//! # Why
//! The async `EmbeddingProvider` used by the pipeline cannot be called from
//! the sync `Chunker::chunk` method directly. A dedicated synchronous trait
//! keeps the chunker agnostic to async runtimes and lets us plug in local
//! ONNX models (via the `fastembed` feature) without an async interface.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SemanticError {
    #[error("signal provider failed: {0}")]
    Provider(String),
    #[error("sentence segmentation produced zero sentences for non-empty input")]
    NoSentences,
    #[error("config invalid: {0}")]
    InvalidConfig(String),
}

/// Blocking embedding source used by [`super::SemanticChunker`].
pub trait SemanticSignalProvider: Send + Sync {
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError>;

    /// Stable provider identity embedded into strategy fingerprints.
    /// Examples: `"openai:text-embedding-3-small"`, `"fastembed:all-MiniLM-L6-v2"`.
    fn fingerprint(&self) -> &str;
}
