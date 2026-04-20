//! Text chunking (Phase 1).
//!
//! # Why
//! Chunking is a first-class, configurable transform in the ingestion pipeline.
//! Phase 1 introduces a pluggable [`Chunker`] trait, token-based sizing, and a
//! [`StrategyFingerprint`] that is mixed into point IDs to keep determinism
//! safe across future strategy evolutions.

mod error;
mod fingerprint;
pub mod size;
mod engine;
pub mod recursive;
mod legacy;
mod public_types;

pub use error::{ChunkError, ChunkResult};
pub use fingerprint::StrategyFingerprint;
pub use public_types::{BoundaryKind, Chunk, ChunkedDocument};
pub use size::{CharCounter, SizeMetric, TiktokenCounter, TokenCounter};
pub use recursive::RecursiveChunker;

#[allow(deprecated)]
pub use legacy::{chunk_document, chunk_text, ChunkerConfig, ChunkingStrategy};

/// Abstract chunker.
pub trait Chunker: Send + Sync {
    fn chunk(&self, text: &str) -> ChunkResult<ChunkedDocument>;
    fn strategy_fingerprint(&self) -> &StrategyFingerprint;
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
