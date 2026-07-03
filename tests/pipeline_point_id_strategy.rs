//! Proves that distinct chunker strategies produce DISJOINT point-ID sets on
//! the same document. This guards against silent collisions after future
//! chunker-parameter upgrades (the whole point of the strategy fingerprint).

use std::sync::Arc;

use async_trait::async_trait;

use ragloom::RagloomError;
use ragloom::doc::{DocumentLoader, LoadedDocument};
use ragloom::embed::EmbeddingProvider;
use ragloom::ids::FileFingerprint;
use ragloom::sink::{Sink, VectorPoint};
use ragloom::transform::chunker::{
    Chunker,
    recursive::{RecursiveChunker, RecursiveConfig},
    size::SizeMetric,
};

fn build_chunker(max: usize) -> Arc<dyn Chunker> {
    Arc::new(
        RecursiveChunker::new(RecursiveConfig {
            metric: SizeMetric::Chars,
            max_size: max,
            min_size: 0,
            overlap: 0,
        })
        .expect("config valid"),
    )
}

#[derive(Debug, Default)]
struct FakeEmbedding;

#[async_trait]
impl EmbeddingProvider for FakeEmbedding {
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, RagloomError> {
        // Deterministic zero vectors — we only care about IDs in this test.
        Ok(inputs.iter().map(|_| vec![0.0_f32, 0.0_f32]).collect())
    }
}

#[derive(Debug, Default)]
struct FakeSink;

#[async_trait]
impl Sink for FakeSink {
    async fn upsert_points(&self, _points: Vec<VectorPoint>) -> Result<(), RagloomError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct FakeLoader;

#[async_trait]
impl DocumentLoader for FakeLoader {
    async fn load(&self, _path: &str) -> Result<LoadedDocument, RagloomError> {
        Ok(LoadedDocument {
            text: String::new(),
        })
    }
}

#[tokio::test]
async fn two_strategies_yield_distinct_point_ids() {
    let fp = FileFingerprint {
        canonical_path: "/tmp/x.txt".into(),
        size_bytes: 10,
        mtime_unix_secs: 1,
        etag: None,
    };
    let text = "hello world. good morning. nice weather. lots of content here. \
                and more prose. even more. still more. and yet more.";

    let exec_a = ragloom::pipeline::runtime::PipelineExecutor::with_chunker(
        Arc::new(FakeEmbedding),
        Arc::new(FakeSink),
        Arc::new(FakeLoader),
        build_chunker(16),
    );
    let exec_b = ragloom::pipeline::runtime::PipelineExecutor::with_chunker(
        Arc::new(FakeEmbedding),
        Arc::new(FakeSink),
        Arc::new(FakeLoader),
        build_chunker(32),
    );

    let points_a = exec_a.build_points_from_text(&fp, text).await.expect("a");
    let points_b = exec_b.build_points_from_text(&fp, text).await.expect("b");

    // Collect ID strings into sets. Disjoint means strategies didn't silently
    // collide.
    let ids_a: std::collections::HashSet<String> =
        points_a.iter().map(|p| format!("{:?}", p.id)).collect();
    let ids_b: std::collections::HashSet<String> =
        points_b.iter().map(|p| format!("{:?}", p.id)).collect();

    assert!(
        !ids_a.is_empty() && !ids_b.is_empty(),
        "both chunkers must produce at least one point"
    );
    assert!(
        ids_a.is_disjoint(&ids_b),
        "IDs must differ across strategies but overlap: a={:?} b={:?}",
        ids_a,
        ids_b
    );
    assert_eq!(
        points_a[0].id.as_str(),
        "9904e630-a206-4da1-b15f-aa169b35f7de",
        "v1 must preserve the point-ID algorithm released by v0.5"
    );
}
