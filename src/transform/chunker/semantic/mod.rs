//! Semantic (similarity-based) chunking.
//!
//! Phase 3 adds a sync [`SemanticSignalProvider`] abstraction, a sync-over-async
//! [`EmbeddingProviderAdapter`] that bridges to the existing async
//! `EmbeddingProvider`, and a [`SemanticChunker`] that splits text at
//! percentile-threshold peaks in adjacent-sentence cosine distance.

pub mod signal;
pub mod adapter;
pub mod sentence;

#[cfg(feature = "fastembed")]
pub mod fastembed;

pub use signal::{SemanticError, SemanticSignalProvider};
pub use adapter::EmbeddingProviderAdapter;

#[cfg(feature = "fastembed")]
pub use fastembed::FastembedSignalProvider;
