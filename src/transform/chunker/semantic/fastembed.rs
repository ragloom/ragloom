//! Placeholder for Task 5 — real `FastembedSignalProvider` lands later.

use super::signal::{SemanticError, SemanticSignalProvider};

/// Placeholder — real struct ships in Task 5.
pub struct FastembedSignalProvider {
    _fingerprint: String,
}

impl SemanticSignalProvider for FastembedSignalProvider {
    fn embed(&self, _inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
        Err(SemanticError::Provider(
            "fastembed provider not yet implemented (Task 5)".into(),
        ))
    }
    fn fingerprint(&self) -> &str {
        &self._fingerprint
    }
}
