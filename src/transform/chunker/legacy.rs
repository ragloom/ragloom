//! Legacy chunker API — thin deprecated adapters over [`RecursiveChunker`].
//!
//! # Why
//! External callers already import `chunk_text` / `chunk_document` /
//! `ChunkerConfig`. Phase 1 keeps those symbols working but routes them
//! through the new recursive implementation so we don't maintain two
//! algorithms.

use super::public_types::{BoundaryKind, Chunk, ChunkedDocument};
use super::recursive::{RecursiveChunker, RecursiveConfig};
use super::size::SizeMetric;
use super::Chunker;

/// Chunker configuration (deprecated shim).
///
/// # Why
/// Keeping parameters explicit makes chunk boundaries reproducible, which is
/// required for deterministic IDs and idempotent sink writes.
#[deprecated(
    since = "0.2.0",
    note = "use RecursiveChunker through the Chunker trait"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkerConfig {
    pub max_chars: usize,
    pub min_chars: usize,
    pub overlap_chars: usize,
    #[allow(deprecated)]
    pub strategy: ChunkingStrategy,
}

#[allow(deprecated)]
impl ChunkerConfig {
    /// Compatibility constructor (simple max-char chunking, no overlap).
    #[deprecated(
        since = "0.2.0",
        note = "use RecursiveChunker through the Chunker trait"
    )]
    pub fn new(max_chars: usize) -> Self {
        Self {
            max_chars,
            min_chars: 0,
            overlap_chars: 0,
            strategy: ChunkingStrategy::BoundaryAware,
        }
    }
}

#[deprecated(
    since = "0.2.0",
    note = "use RecursiveChunker through the Chunker trait"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkingStrategy {
    BoundaryAware,
}

#[allow(deprecated)]
fn to_recursive(cfg: &ChunkerConfig) -> RecursiveConfig {
    RecursiveConfig {
        metric: SizeMetric::Chars,
        max_size: cfg.max_chars,
        min_size: cfg.min_chars,
        overlap: cfg.overlap_chars,
    }
}

/// Splits a document into chunks (deprecated shim).
#[deprecated(
    since = "0.2.0",
    note = "use RecursiveChunker through the Chunker trait"
)]
#[allow(deprecated)]
pub fn chunk_document(text: &str, cfg: &ChunkerConfig) -> ChunkedDocument {
    let rec = match RecursiveChunker::new(to_recursive(cfg)) {
        Ok(r) => r,
        Err(_) => return ChunkedDocument { chunks: Vec::new() },
    };
    rec.chunk(text).unwrap_or(ChunkedDocument { chunks: Vec::new() })
}

/// Splits `text` into chunks using a simple character budget (deprecated shim).
#[deprecated(
    since = "0.2.0",
    note = "use RecursiveChunker through the Chunker trait"
)]
#[allow(deprecated)]
pub fn chunk_text(text: &str, cfg: ChunkerConfig) -> Vec<String> {
    chunk_document(text, &cfg)
        .chunks
        .into_iter()
        .map(|c| c.text)
        .collect()
}

// Silence unused-import warnings; `BoundaryKind` and `Chunk` are retained as
// public re-exports through the module root.
#[allow(dead_code, unused_imports)]
fn _keep_types_alive(_: BoundaryKind, _: Chunk) {}

#[cfg(test)]
mod tests {
    #![allow(deprecated)]
    use super::*;

    #[test]
    fn empty_input_produces_no_chunks() {
        assert!(chunk_text("", ChunkerConfig::new(10)).is_empty());
    }

    #[test]
    fn zero_budget_produces_no_chunks() {
        assert!(chunk_text("hello", ChunkerConfig::new(0)).is_empty());
    }

    #[test]
    fn splits_into_fixed_size_chunks() {
        let chunks = chunk_text("abcdefghij", ChunkerConfig::new(4));
        assert_eq!(chunks, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn multibyte_unicode_does_not_panic() {
        let chunks = chunk_text("你好世界🙂hello", ChunkerConfig::new(3));
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(!c.is_empty());
        }
    }

    #[test]
    fn boundary_preference_prefers_paragraph_break() {
        let cfg = ChunkerConfig {
            max_chars: 8,
            min_chars: 0,
            overlap_chars: 0,
            strategy: ChunkingStrategy::BoundaryAware,
        };
        let doc = chunk_document("aaaa\n\nbbbb cccc\ndddd", &cfg);
        assert!(doc.chunks.len() >= 2);
        assert_eq!(doc.chunks[0].text, "aaaa");
    }

    #[test]
    fn overlap_moves_next_start_backward() {
        let cfg = ChunkerConfig {
            max_chars: 5,
            min_chars: 0,
            overlap_chars: 2,
            strategy: ChunkingStrategy::BoundaryAware,
        };
        let texts: Vec<String> = chunk_text("abcdefghij", cfg);
        assert_eq!(texts, vec!["abcde", "defgh", "ghij"]);
    }
}
