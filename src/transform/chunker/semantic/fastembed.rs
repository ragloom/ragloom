//! Local ONNX-based signal provider backed by the `fastembed` crate.
//!
//! # Why
//! Calling the pipeline's hosted `EmbeddingProvider` once per sentence inflates
//! API cost for semantic chunking. `fastembed` runs a small ONNX model locally
//! (default: `sentence-transformers/all-MiniLM-L6-v2`, 384 dim), giving zero
//! per-call cost at the price of ~25 MB of model weights and an ort runtime.

use std::sync::Mutex;

// NOTE: `InitOptions` is the deprecated alias for `TextInitOptions` in
// fastembed 5.x; we use `TextInitOptions` directly to avoid the deprecation
// warning.
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use super::signal::{SemanticError, SemanticSignalProvider};

pub struct FastembedSignalProvider {
    // `TextEmbedding::embed` requires `&mut self` (it mutates internal ORT
    // session state), while `SemanticSignalProvider::embed` takes `&self`.
    // A blocking `Mutex` is fine: semantic chunking serialises embedding
    // calls per-document, and the inner work is CPU-bound.
    model: Mutex<TextEmbedding>,
    fingerprint: &'static str,
}

impl FastembedSignalProvider {
    pub fn new() -> Result<Self, SemanticError> {
        let model = TextEmbedding::try_new(TextInitOptions::new(EmbeddingModel::AllMiniLML6V2))
            .map_err(|e| SemanticError::Provider(format!("fastembed init: {e}")))?;
        Ok(Self {
            model: Mutex::new(model),
            fingerprint: "fastembed:all-MiniLM-L6-v2",
        })
    }
}

impl SemanticSignalProvider for FastembedSignalProvider {
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
        let mut guard = self
            .model
            .lock()
            .map_err(|e| SemanticError::Provider(format!("fastembed lock poisoned: {e}")))?;
        guard
            .embed(inputs, None)
            .map_err(|e| SemanticError::Provider(format!("fastembed embed: {e}")))
    }

    fn fingerprint(&self) -> &str {
        self.fingerprint
    }
}
