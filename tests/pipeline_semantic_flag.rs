//! Verifies that `semantic_router` and `default_router` produce disjoint
//! point-ID sets for the same `.md` / `.txt` input.

use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;

use ragloom::RagloomError;
use ragloom::doc::DocumentLoader;
use ragloom::embed::EmbeddingProvider;
use ragloom::ids::FileFingerprint;
use ragloom::sink::{Sink, VectorPoint};
use ragloom::transform::chunker::semantic::{
    SemanticChunker, SemanticSignalProvider, signal::SemanticError,
};
use ragloom::transform::chunker::{
    Chunker, default_router, recursive::RecursiveConfig, semantic_router, size::SizeMetric,
};

#[derive(Default)]
struct FakeEmbedding;
#[async_trait]
impl EmbeddingProvider for FakeEmbedding {
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, RagloomError> {
        Ok(inputs.iter().map(|_| vec![0.0; 4]).collect())
    }
}
#[derive(Default)]
struct FakeSink;
#[async_trait]
impl Sink for FakeSink {
    async fn upsert_points(&self, _: Vec<VectorPoint>) -> Result<(), RagloomError> {
        Ok(())
    }
}
#[derive(Default)]
struct FakeLoader;
#[async_trait]
impl DocumentLoader for FakeLoader {
    async fn load_utf8(&self, _: &str) -> Result<String, RagloomError> {
        Ok(String::new())
    }
}

struct StubSignal;
impl SemanticSignalProvider for StubSignal {
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
        Ok(inputs.iter().map(|_| vec![1.0_f32, 0.0]).collect())
    }
    fn fingerprint(&self) -> &str {
        "stub:flag"
    }
}

fn cfg() -> RecursiveConfig {
    RecursiveConfig {
        metric: SizeMetric::Chars,
        max_size: 64,
        min_size: 0,
        overlap: 0,
    }
}

fn exec(router: Arc<dyn Chunker>) -> ragloom::pipeline::runtime::PipelineExecutor {
    ragloom::pipeline::runtime::PipelineExecutor::with_chunker(
        Arc::new(FakeEmbedding),
        Arc::new(FakeSink),
        Arc::new(FakeLoader),
        router,
    )
}

#[tokio::test]
async fn default_and_semantic_routers_produce_disjoint_ids_for_md() {
    let text = "# hi\n\nFirst sentence. Second sentence. Third sentence.\n";
    let fp = FileFingerprint {
        canonical_path: "/tmp/n.md".into(),
        size_bytes: 1,
        mtime_unix_secs: 1,
    };

    let default: Arc<dyn Chunker> = Arc::new(default_router(cfg()).unwrap());
    let semantic_c: Arc<dyn Chunker> =
        Arc::new(SemanticChunker::new(Arc::new(StubSignal), cfg(), 95).unwrap());
    let semantic: Arc<dyn Chunker> = Arc::new(semantic_router(cfg(), semantic_c).unwrap());

    let default_pts = exec(default)
        .build_points_from_text(&fp, text)
        .await
        .unwrap();
    let semantic_pts = exec(semantic)
        .build_points_from_text(&fp, text)
        .await
        .unwrap();

    let a: HashSet<String> = default_pts.iter().map(|p| format!("{:?}", p.id)).collect();
    let b: HashSet<String> = semantic_pts.iter().map(|p| format!("{:?}", p.id)).collect();

    assert!(!a.is_empty() && !b.is_empty());
    assert!(a.is_disjoint(&b), "expected disjoint: a={a:?} b={b:?}");
}
