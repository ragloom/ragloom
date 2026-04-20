//! Text chunking (Phase 1).
//!
//! # Why
//! Chunking is a first-class, configurable transform in the ingestion pipeline.
//! Phase 1 introduces a pluggable [`Chunker`] trait, token-based sizing, and a
//! [`StrategyFingerprint`] that is mixed into point IDs to keep determinism
//! safe across future strategy evolutions.

pub mod code;
mod engine;
mod error;
mod fingerprint;
mod legacy;
pub mod markdown;
mod public_types;
pub mod recursive;
pub mod router;
pub mod semantic;
pub mod size;

pub use code::{CodeChunker, Language};
pub use error::{ChunkError, ChunkResult};
pub use fingerprint::StrategyFingerprint;
pub use markdown::MarkdownChunker;
pub use public_types::{BoundaryKind, Chunk, ChunkedDocument};
pub use recursive::RecursiveChunker;
pub use router::{ChunkerRouter, default_router, semantic_router};
#[cfg(feature = "fastembed")]
pub use semantic::FastembedSignalProvider;
pub use semantic::{
    EmbeddingProviderAdapter, SemanticChunker, SemanticConfig, SemanticError,
    SemanticSignalProvider,
};
pub use size::{CharCounter, SizeMetric, TiktokenCounter, TokenCounter};

#[allow(deprecated)]
pub use legacy::{ChunkerConfig, ChunkingStrategy, chunk_document, chunk_text};

/// Per-call metadata that a [`Chunker`] may use to choose behaviour.
///
/// # Why
/// Content-type-aware chunkers need the file path or extension to decide which
/// strategy to apply. Keeping this information on the method call instead of
/// the chunker's construction preserves [`Chunker`] as a stateless strategy.
#[derive(Debug, Clone, Default)]
pub struct ChunkHint<'a> {
    pub path: Option<&'a str>,
    pub extension: Option<&'a str>,
    pub mime: Option<&'a str>,
}

impl<'a> ChunkHint<'a> {
    /// An empty hint. Use in tests or when the caller truly has no context.
    pub fn none() -> Self {
        Self::default()
    }

    /// Build a hint from a canonical path. Extension is the last segment
    /// after the final `.`; returned borrowed from the input slice. Returns
    /// no extension for dotfiles (e.g. `.gitignore`) and files with no `.`.
    pub fn from_path(path: &'a str) -> Self {
        let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let extension = filename
            .rsplit_once('.')
            .and_then(|(stem, ext)| if stem.is_empty() { None } else { Some(ext) });
        Self {
            path: Some(path),
            extension,
            mime: None,
        }
    }
}

pub trait Chunker: Send + Sync {
    fn chunk(&self, text: &str, hint: &ChunkHint<'_>) -> ChunkResult<ChunkedDocument>;
}

/// Default recursive config used by [`crate::pipeline::runtime::PipelineExecutor::new`]:
/// chars metric, 512-char budget, no overlap, no min-size floor.
pub fn recursive_config_chars_512() -> recursive::RecursiveConfig {
    recursive::RecursiveConfig {
        metric: size::SizeMetric::Chars,
        max_size: 512,
        min_size: 0,
        overlap: 0,
    }
}

#[cfg(test)]
mod hint_tests {
    use super::ChunkHint;

    #[test]
    fn extracts_extension_from_unix_path() {
        let h = ChunkHint::from_path("/a/b/c.md");
        assert_eq!(h.extension, Some("md"));
        assert_eq!(h.path, Some("/a/b/c.md"));
    }

    #[test]
    fn extracts_extension_from_windows_path() {
        let h = ChunkHint::from_path("d:\\code\\main.rs");
        assert_eq!(h.extension, Some("rs"));
    }

    #[test]
    fn no_extension_for_dotfiles() {
        let h = ChunkHint::from_path("/repo/.gitignore");
        assert_eq!(h.extension, None);
    }

    #[test]
    fn no_extension_when_no_dot() {
        let h = ChunkHint::from_path("/repo/Makefile");
        assert_eq!(h.extension, None);
    }

    #[test]
    fn multi_segment_takes_last() {
        let h = ChunkHint::from_path("archive.tar.gz");
        assert_eq!(h.extension, Some("gz"));
    }
}
