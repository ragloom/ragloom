//! End-to-end SemanticChunker integration using a deterministic mock provider.
//!
//! Verifies that topic shifts in the sample document produce distinct chunks
//! and the strategy fingerprint is always `semantic:v1|…`.

use std::sync::Arc;

use ragloom::transform::chunker::semantic::{
    SemanticChunker, SemanticSignalProvider, signal::SemanticError,
};
use ragloom::transform::chunker::{
    ChunkHint, Chunker, recursive::RecursiveConfig, size::SizeMetric,
};

struct KeywordSignal;

fn bucket(s: &str) -> [f32; 3] {
    let first = s.split_whitespace().next().unwrap_or("").to_lowercase();
    if first.starts_with("cat") || first.starts_with("the") {
        [1.0, 0.0, 0.0]
    } else if first.starts_with("rust") || first.starts_with("its") || first.starts_with("many") {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    }
}

impl SemanticSignalProvider for KeywordSignal {
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
        Ok(inputs.iter().map(|s| bucket(s).to_vec()).collect())
    }
    fn fingerprint(&self) -> &str {
        "keyword:test"
    }
}

fn cfg() -> RecursiveConfig {
    RecursiveConfig {
        metric: SizeMetric::Chars,
        max_size: 2000,
        min_size: 0,
        overlap: 0,
    }
}

#[test]
fn three_topics_produce_multiple_chunks() {
    let text = std::fs::read_to_string("tests/fixtures/semantic/sample.txt").unwrap();
    let signal: Arc<dyn SemanticSignalProvider> = Arc::new(KeywordSignal);
    let c = SemanticChunker::new(signal, cfg(), 60).unwrap();
    let doc = c.chunk(&text, &ChunkHint::none()).unwrap();

    assert!(
        doc.chunks.len() >= 2,
        "expected multi-chunk split, got: {:?}",
        doc.chunks
    );
    assert!(doc.strategy_fingerprint.as_str().starts_with("semantic:v1"));
    assert!(
        doc.strategy_fingerprint
            .as_str()
            .contains("signal=keyword:test")
    );
    assert!(doc.strategy_fingerprint.as_str().contains("percentile=60"));
}

#[test]
fn fingerprint_changes_with_percentile() {
    let signal1: Arc<dyn SemanticSignalProvider> = Arc::new(KeywordSignal);
    let signal2: Arc<dyn SemanticSignalProvider> = Arc::new(KeywordSignal);
    let c1 = SemanticChunker::new(signal1, cfg(), 50).unwrap();
    let c2 = SemanticChunker::new(signal2, cfg(), 95).unwrap();
    assert_ne!(c1.fingerprint().as_str(), c2.fingerprint().as_str());
}
