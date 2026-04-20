//! Semantic (similarity-based) chunking.
//!
//! Phase 3 adds a sync [`SemanticSignalProvider`] abstraction, a sync-over-async
//! [`EmbeddingProviderAdapter`] that bridges to the existing async
//! `EmbeddingProvider`, and a [`SemanticChunker`] that splits text at
//! percentile-threshold peaks in adjacent-sentence cosine distance.

pub mod adapter;
pub mod sentence;
pub mod signal;

#[cfg(feature = "fastembed")]
pub mod fastembed;

pub use adapter::EmbeddingProviderAdapter;
pub use signal::{SemanticError, SemanticSignalProvider};

#[cfg(feature = "fastembed")]
pub use fastembed::FastembedSignalProvider;

use std::sync::Arc;

use super::error::{ChunkError, ChunkResult};
use super::fingerprint::StrategyFingerprint;
use super::recursive::{RecursiveChunker, RecursiveConfig};
use super::size::SizeMetric;
use super::{Chunk, ChunkHint, ChunkedDocument, Chunker};

use self::sentence::{Sentence, sentences};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticConfig {
    pub metric: SizeMetric,
    pub max_size: usize,
    pub min_size: usize,
    /// 1..=99. 95 means "split where adjacent-sentence distance is in the top 5 %".
    pub percentile: u8,
}

impl SemanticConfig {
    pub fn validate(&self) -> Result<(), SemanticError> {
        if self.percentile == 0 || self.percentile > 99 {
            return Err(SemanticError::InvalidConfig(format!(
                "percentile must be in 1..=99, got {}",
                self.percentile
            )));
        }
        if self.min_size > self.max_size {
            return Err(SemanticError::InvalidConfig(format!(
                "min_size ({}) > max_size ({})",
                self.min_size, self.max_size
            )));
        }
        Ok(())
    }
}

pub struct SemanticChunker {
    signal: Arc<dyn SemanticSignalProvider>,
    inner: Arc<RecursiveChunker>,
    counter: Arc<dyn super::size::TokenCounter>,
    config: SemanticConfig,
    fingerprint: StrategyFingerprint,
}

impl SemanticChunker {
    pub fn new(
        signal: Arc<dyn SemanticSignalProvider>,
        size_cfg: RecursiveConfig,
        percentile: u8,
    ) -> ChunkResult<Self> {
        let config = SemanticConfig {
            metric: size_cfg.metric,
            max_size: size_cfg.max_size,
            min_size: size_cfg.min_size,
            percentile,
        };
        config.validate().map_err(ChunkError::Semantic)?;

        let inner = Arc::new(RecursiveChunker::new(size_cfg)?);
        let counter = super::size::counter_for(size_cfg.metric)?;
        let metric_str = match config.metric {
            SizeMetric::Chars => "chars",
            SizeMetric::Tokens => "tokens",
        };
        let fp = format!(
            "semantic:v1|signal={}|metric={}|max={}|min={}|percentile={}",
            signal.fingerprint(),
            metric_str,
            config.max_size,
            config.min_size,
            config.percentile,
        );
        Ok(Self {
            signal,
            inner,
            counter,
            config,
            fingerprint: StrategyFingerprint::new(fp),
        })
    }

    pub fn fingerprint(&self) -> &StrategyFingerprint {
        &self.fingerprint
    }

    fn finalise(&self, chunks: Vec<Chunk>) -> ChunkResult<ChunkedDocument> {
        Ok(ChunkedDocument {
            chunks,
            strategy_fingerprint: self.fingerprint.clone(),
        })
    }

    fn fallback_recursive(&self, text: &str) -> ChunkResult<ChunkedDocument> {
        // If the whole text already fits the size budget, emit it as a single
        // semantic chunk so fallback for tiny inputs doesn't trigger
        // RecursiveChunker's greedy whitespace splitting.
        if self.counter.count(text) <= self.config.max_size {
            let char_len = text.chars().count();
            let chunk = Chunk {
                index: 0,
                text: text.to_string(),
                boundary: super::BoundaryKind::Forced,
                start_byte: 0,
                end_byte: text.len(),
                char_len,
            };
            return self.finalise(vec![chunk]);
        }
        let chunks = self.inner.chunk_raw(text)?;
        self.finalise(chunks)
    }
}

impl Chunker for SemanticChunker {
    #[tracing::instrument(
        name = "ragloom.chunker.semantic.chunk",
        skip(self, text, _hint),
        fields(bytes = text.len(), strategy = %self.fingerprint)
    )]
    fn chunk(&self, text: &str, _hint: &ChunkHint<'_>) -> ChunkResult<ChunkedDocument> {
        if text.is_empty() {
            return self.finalise(Vec::new());
        }

        let sentence_list = sentences(text);
        if sentence_list.len() < 2 {
            return self.fallback_recursive(text);
        }

        let inputs: Vec<String> = sentence_list.iter().map(|s| s.text.to_string()).collect();
        let embeds = self.signal.embed(&inputs).map_err(ChunkError::Semantic)?;
        if embeds.len() != sentence_list.len() {
            return Err(ChunkError::Semantic(SemanticError::Provider(format!(
                "count mismatch: expected {}, got {}",
                sentence_list.len(),
                embeds.len()
            ))));
        }

        let unit: Vec<Vec<f32>> = embeds.into_iter().map(normalise_vector).collect();

        let distances: Vec<f32> = unit
            .windows(2)
            .map(|w| 1.0_f32 - dot(&w[0], &w[1]))
            .collect();
        if distances.is_empty() {
            return self.fallback_recursive(text);
        }

        let threshold = percentile(&distances, self.config.percentile);
        // Strict `>` so uniformly-similar distances (threshold equal to the
        // minimum) don't trigger spurious splits at every sentence boundary.
        let split_at: Vec<usize> = distances
            .iter()
            .enumerate()
            .filter(|(_, d)| **d > threshold)
            .map(|(i, _)| i + 1)
            .collect();

        tracing::debug!(
            event.name = "ragloom.chunker.semantic.thresholded",
            sentences = sentence_list.len(),
            splits = split_at.len(),
            threshold = threshold as f64,
            "ragloom.chunker.semantic.thresholded"
        );

        let groups = build_groups(&sentence_list, &split_at);
        let groups = merge_for_min_size(
            groups,
            &sentence_list,
            self.config.min_size,
            self.config.metric,
        );

        let mut chunks: Vec<Chunk> = Vec::new();
        let mut next_index = 0usize;

        for range in groups {
            let start_byte = sentence_list[range.0].start_byte;
            let end_byte = sentence_list[range.1].end_byte;
            let slab = &text[start_byte..end_byte];
            if slab.trim().is_empty() {
                continue;
            }
            // If the semantic group already fits the size budget, emit it as
            // a single chunk. Re-running it through RecursiveChunker would
            // greedily split on whitespace even when the group is small,
            // which defeats the semantic grouping we just computed.
            if self.counter.count(slab) <= self.config.max_size {
                chunks.push(Chunk {
                    index: next_index,
                    text: slab.to_string(),
                    boundary: super::BoundaryKind::Paragraph,
                    start_byte,
                    end_byte,
                    char_len: slab.chars().count(),
                });
                next_index += 1;
                continue;
            }
            let doc = self.inner.chunk(slab, &ChunkHint::none())?;
            for chunk in doc.chunks {
                chunks.push(Chunk {
                    index: next_index,
                    text: chunk.text,
                    boundary: chunk.boundary,
                    start_byte: start_byte + chunk.start_byte,
                    end_byte: start_byte + chunk.end_byte,
                    char_len: chunk.char_len,
                });
                next_index += 1;
            }
        }

        self.finalise(chunks)
    }
}

fn normalise_vector(v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        return v;
    }
    v.into_iter().map(|x| x / norm).collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut s = 0.0_f32;
    for i in 0..n {
        s += a[i] * b[i];
    }
    s
}

/// Linear-interpolation percentile.
fn percentile(values: &[f32], p: u8) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let rank = (p as f32 / 100.0) * (n as f32 - 1.0);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let frac = rank - lo as f32;
        sorted[lo] + frac * (sorted[hi] - sorted[lo])
    }
}

fn build_groups(sentences: &[Sentence<'_>], splits: &[usize]) -> Vec<(usize, usize)> {
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for &split in splits {
        if split == 0 || split >= sentences.len() {
            continue;
        }
        groups.push((start, split - 1));
        start = split;
    }
    if start < sentences.len() {
        groups.push((start, sentences.len() - 1));
    }
    groups
}

fn merge_for_min_size(
    groups: Vec<(usize, usize)>,
    sentences: &[Sentence<'_>],
    min_size: usize,
    metric: SizeMetric,
) -> Vec<(usize, usize)> {
    if min_size == 0 {
        return groups;
    }
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for g in groups {
        let size = group_size(g, sentences, metric);
        if size < min_size
            && let Some(prev) = merged.last_mut()
        {
            prev.1 = g.1;
            continue;
        }
        merged.push(g);
    }
    merged
}

fn group_size(g: (usize, usize), sentences: &[Sentence<'_>], metric: SizeMetric) -> usize {
    let _ = metric; // chars metric only for now
    sentences
        .get(g.0..=g.1)
        .map(|ss| ss.iter().map(|s| s.text.chars().count()).sum::<usize>())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockSignal {
        vectors: Vec<Vec<f32>>,
        fingerprint: &'static str,
    }

    impl MockSignal {
        fn new(vectors: Vec<Vec<f32>>) -> Self {
            Self {
                vectors,
                fingerprint: "mock:test",
            }
        }
    }

    impl SemanticSignalProvider for MockSignal {
        fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
            assert_eq!(inputs.len(), self.vectors.len(), "mock got wrong count");
            Ok(self.vectors.clone())
        }
        fn fingerprint(&self) -> &str {
            self.fingerprint
        }
    }

    fn cfg(max: usize, min: usize) -> RecursiveConfig {
        RecursiveConfig {
            metric: SizeMetric::Chars,
            max_size: max,
            min_size: min,
            overlap: 0,
        }
    }

    #[test]
    fn empty_input_produces_no_chunks() {
        let mock = Arc::new(MockSignal::new(vec![]));
        let c = SemanticChunker::new(mock, cfg(1000, 0), 95).unwrap();
        let doc = c.chunk("", &ChunkHint::none()).unwrap();
        assert!(doc.chunks.is_empty());
        assert!(doc.strategy_fingerprint.as_str().starts_with("semantic:v1"));
    }

    #[test]
    fn single_sentence_falls_back_with_semantic_fingerprint() {
        let mock = Arc::new(MockSignal::new(vec![]));
        let c = SemanticChunker::new(mock, cfg(1000, 0), 95).unwrap();
        let doc = c.chunk("one lonely sentence", &ChunkHint::none()).unwrap();
        assert_eq!(doc.chunks.len(), 1);
        assert!(doc.strategy_fingerprint.as_str().starts_with("semantic:v1"));
    }

    #[test]
    fn highly_similar_sentences_remain_one_group() {
        let vectors = vec![
            vec![1.0, 0.0, 0.0],
            vec![1.0, 0.0, 0.0],
            vec![1.0, 0.0, 0.0],
        ];
        let mock = Arc::new(MockSignal::new(vectors));
        let c = SemanticChunker::new(mock, cfg(1000, 0), 95).unwrap();
        let doc = c.chunk("A. B. C.", &ChunkHint::none()).unwrap();
        assert_eq!(doc.chunks.len(), 1, "got: {:?}", doc.chunks);
    }

    #[test]
    fn topic_shift_produces_two_chunks() {
        let vectors = vec![
            vec![1.0, 0.0, 0.0],
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 1.0, 0.0],
        ];
        let mock = Arc::new(MockSignal::new(vectors));
        let c = SemanticChunker::new(mock, cfg(1000, 0), 50).unwrap();
        let doc = c
            .chunk("First A. First B. Second A. Second B.", &ChunkHint::none())
            .unwrap();
        assert_eq!(doc.chunks.len(), 2, "got: {:?}", doc.chunks);
    }

    #[test]
    fn fingerprint_contains_signal_and_percentile() {
        let mock = Arc::new(MockSignal::new(vec![]));
        let c = SemanticChunker::new(mock, cfg(1000, 0), 77).unwrap();
        let fp = c.fingerprint().as_str();
        assert!(fp.contains("signal=mock:test"));
        assert!(fp.contains("percentile=77"));
        assert!(fp.contains("metric=chars"));
    }

    #[test]
    fn invalid_percentile_rejected() {
        let mock = Arc::new(MockSignal::new(vec![]));
        let e0 = SemanticChunker::new(mock.clone(), cfg(1000, 0), 0);
        assert!(e0.is_err());
        let e100 = SemanticChunker::new(mock, cfg(1000, 0), 100);
        assert!(e100.is_err());
    }

    #[test]
    fn provider_count_mismatch_surfaces_as_error() {
        struct LooseMock;
        impl SemanticSignalProvider for LooseMock {
            fn embed(&self, _: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
                Ok(vec![vec![1.0, 0.0]])
            }
            fn fingerprint(&self) -> &str {
                "loose"
            }
        }
        let c = SemanticChunker::new(Arc::new(LooseMock), cfg(1000, 0), 50).unwrap();
        let err = c
            .chunk("A. B. C.", &ChunkHint::none())
            .expect_err("mismatch");
        assert!(matches!(err, ChunkError::Semantic(_)));
    }

    #[test]
    fn percentile_linear_interpolation_matches_known_values() {
        let v = vec![0.0_f32, 0.25, 0.5, 0.75, 1.0];
        assert!((percentile(&v, 50) - 0.5).abs() < 1e-6);
        assert!((percentile(&v, 25) - 0.25).abs() < 1e-6);
        assert!((percentile(&v, 95) - 0.95).abs() < 1e-6);
    }
}
