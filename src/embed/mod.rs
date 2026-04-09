//! Embedding providers.
//!
//! # Why
//! Embedding is an external dependency boundary. We keep it behind a trait so
//! the pipeline can remain open to new providers (HTTP API, local inference)
//! without changing call sites.

pub mod http_client;
pub mod openai_client;

use async_trait::async_trait;

use crate::error::RagloomError;

/// Produces embedding vectors for input texts.
///
/// # Why
/// The pipeline needs a stable interface for embedding so execution and retry
/// policy can be tested independently of any specific vendor API.
#[async_trait]
pub trait EmbeddingProvider {
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, RagloomError>;
}
