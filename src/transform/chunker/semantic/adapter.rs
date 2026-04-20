//! Sync-over-async adapter bridging [`SemanticSignalProvider`] to the
//! async [`crate::embed::EmbeddingProvider`] used by the ingestion pipeline.
//!
//! # Why
//! `Chunker::chunk` is synchronous (Phase 2 contract) but the pipeline's
//! embedding backends are async. Rather than breaking the trait, we funnel
//! async calls through a captured Tokio `Handle` with `block_in_place`.
//! When no multi-thread runtime is available (e.g. plain unit tests), we
//! fall back to an owned current-thread runtime.

use std::sync::Arc;

use tokio::runtime::{Handle, Runtime, RuntimeFlavor};

use super::signal::{SemanticError, SemanticSignalProvider};

pub struct EmbeddingProviderAdapter {
    provider: Arc<dyn crate::embed::EmbeddingProvider + Send + Sync>,
    fingerprint: String,
    runtime: AdapterRuntime,
}

enum AdapterRuntime {
    /// Use the caller's multi-thread runtime. Safe with `block_in_place`.
    External(Handle),
    /// Own a dedicated runtime. Used when no compatible ambient runtime exists.
    Owned(Runtime),
}

impl EmbeddingProviderAdapter {
    pub fn new(
        provider: Arc<dyn crate::embed::EmbeddingProvider + Send + Sync>,
        fingerprint: impl Into<String>,
    ) -> Self {
        let runtime = match Handle::try_current() {
            Ok(h) if h.runtime_flavor() == RuntimeFlavor::MultiThread => {
                AdapterRuntime::External(h)
            }
            _ => AdapterRuntime::Owned(
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build owned adapter runtime"),
            ),
        };
        Self {
            provider,
            fingerprint: fingerprint.into(),
            runtime,
        }
    }
}

impl SemanticSignalProvider for EmbeddingProviderAdapter {
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
        let provider = Arc::clone(&self.provider);
        let inputs = inputs.to_vec();
        let fut = async move { provider.embed(&inputs).await };
        let result = match &self.runtime {
            AdapterRuntime::External(h) => tokio::task::block_in_place(|| h.block_on(fut)),
            AdapterRuntime::Owned(rt) => rt.block_on(fut),
        };
        result.map_err(|e| SemanticError::Provider(format!("{e}")))
    }

    fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::EmbeddingProvider;
    use crate::error::RagloomError;
    use async_trait::async_trait;

    struct FakeEmbed(usize);
    #[async_trait]
    impl EmbeddingProvider for FakeEmbed {
        async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, RagloomError> {
            Ok(inputs
                .iter()
                .map(|s| vec![s.len() as f32; self.0])
                .collect())
        }
    }

    #[test]
    fn owned_runtime_fallback_embeds_in_plain_context() {
        let adapter = EmbeddingProviderAdapter::new(Arc::new(FakeEmbed(4)), "fake:test");
        let out = adapter.embed(&["abc".into(), "defg".into()]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], vec![3.0, 3.0, 3.0, 3.0]);
        assert_eq!(out[1], vec![4.0, 4.0, 4.0, 4.0]);
        assert_eq!(adapter.fingerprint(), "fake:test");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn external_runtime_bridges_through_block_on() {
        let adapter = EmbeddingProviderAdapter::new(Arc::new(FakeEmbed(2)), "fake:multi");
        let out = tokio::task::spawn_blocking(move || adapter.embed(&["xy".into()]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out, vec![vec![2.0, 2.0]]);
    }

    #[test]
    fn propagates_provider_error() {
        struct Broken;
        #[async_trait]
        impl EmbeddingProvider for Broken {
            async fn embed(&self, _: &[String]) -> Result<Vec<Vec<f32>>, RagloomError> {
                Err(
                    RagloomError::from_kind(crate::error::RagloomErrorKind::Internal)
                        .with_context("boom"),
                )
            }
        }
        let adapter = EmbeddingProviderAdapter::new(Arc::new(Broken), "x");
        let err = adapter.embed(&["a".into()]).expect_err("must propagate");
        assert!(matches!(err, SemanticError::Provider(_)));
    }
}
