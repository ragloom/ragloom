//! Placeholder for Task 3.

use std::sync::Arc;

use super::signal::{SemanticError, SemanticSignalProvider};

/// Placeholder — real struct ships in Task 3.
pub struct EmbeddingProviderAdapter {
    _provider: Arc<dyn crate::embed::EmbeddingProvider + Send + Sync>,
    _fingerprint: String,
}

impl SemanticSignalProvider for EmbeddingProviderAdapter {
    fn embed(&self, _inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
        Err(SemanticError::Provider("adapter not yet implemented (Task 3)".into()))
    }
    fn fingerprint(&self) -> &str {
        &self._fingerprint
    }
}
