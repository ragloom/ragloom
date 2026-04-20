//! Chunker error types.
//!
//! # Why
//! Errors from chunker construction (bad config, tokenizer init) must surface
//! synchronously at wiring time, not hide inside async pipeline workers.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ChunkError {
    #[error("invalid chunker config: {0}")]
    InvalidConfig(String),
    #[error("tokenizer init failed: {0}")]
    Tokenizer(String),
    #[error("parse error in {lang} at byte {pos}: {detail}")]
    ParseError {
        lang: String,
        pos: usize,
        detail: String,
    },
    #[error("semantic chunker: {0}")]
    Semantic(#[from] crate::transform::chunker::semantic::signal::SemanticError),
}

pub type ChunkResult<T> = Result<T, ChunkError>;
