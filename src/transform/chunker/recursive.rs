//! Recursive delimiter-based chunker.
//!
//! # Why
//! Embedding models prefer medium-sized, semantically coherent inputs. The
//! recursive chunker tries the strongest boundary (paragraph) first and falls
//! back down the hierarchy (line → sentence → whitespace → forced) when
//! necessary, while staying within a configurable size budget.

use std::sync::Arc;

use super::engine::{
    forced_boundary, normalize_newlines, scan_boundaries, Boundary,
    BoundaryKind as EngBoundaryKind,
};
use super::error::{ChunkError, ChunkResult};
use super::fingerprint::StrategyFingerprint;
use super::size::{counter_for, SizeMetric, TokenCounter};
use super::{Chunk, ChunkedDocument, Chunker};

fn map_kind(k: EngBoundaryKind) -> super::BoundaryKind {
    use super::BoundaryKind as B;
    match k {
        EngBoundaryKind::Paragraph => B::Paragraph,
        EngBoundaryKind::Line | EngBoundaryKind::Sentence => B::Line,
        EngBoundaryKind::Whitespace => B::Whitespace,
        EngBoundaryKind::Forced => B::Forced,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecursiveConfig {
    pub metric: SizeMetric,
    pub max_size: usize,
    pub min_size: usize,
    pub overlap: usize,
}

impl RecursiveConfig {
    pub fn validate(&self) -> ChunkResult<()> {
        if self.min_size > self.max_size {
            return Err(ChunkError::InvalidConfig(format!(
                "min_size ({}) > max_size ({})",
                self.min_size, self.max_size
            )));
        }
        if self.max_size > 0 && self.overlap >= self.max_size {
            return Err(ChunkError::InvalidConfig(format!(
                "overlap ({}) must be < max_size ({})",
                self.overlap, self.max_size
            )));
        }
        Ok(())
    }
}

pub struct RecursiveChunker {
    cfg: RecursiveConfig,
    counter: Arc<dyn TokenCounter>,
    fingerprint: StrategyFingerprint,
}

impl RecursiveChunker {
    pub fn new(cfg: RecursiveConfig) -> ChunkResult<Self> {
        cfg.validate()?;
        let counter = counter_for(cfg.metric)?;
        let metric_str = match cfg.metric {
            SizeMetric::Chars => "chars",
            SizeMetric::Tokens => "tokens",
        };
        let fp = format!(
            "recursive:v1|metric={}|tokenizer={}|max={}|min={}|overlap={}",
            metric_str,
            counter.fingerprint(),
            cfg.max_size,
            cfg.min_size,
            cfg.overlap,
        );
        Ok(Self {
            cfg,
            counter,
            fingerprint: StrategyFingerprint::new(fp),
        })
    }
}

impl Chunker for RecursiveChunker {
    #[tracing::instrument(
        name = "ragloom.chunker.recursive.chunk",
        skip(self, text),
        fields(
            bytes = text.len(),
            strategy = %self.fingerprint,
        )
    )]
    fn chunk(&self, text: &str) -> ChunkResult<ChunkedDocument> {
        if self.cfg.max_size == 0 || text.is_empty() {
            return Ok(ChunkedDocument { chunks: Vec::new() });
        }

        let normalized = normalize_newlines(text).into_owned();

        let mut chunks: Vec<Chunk> = Vec::new();
        let mut cursor_byte = 0usize;
        let mut chunk_index = 0usize;

        while cursor_byte < normalized.len() {
            let window_end_byte = advance_to_budget(
                &normalized,
                cursor_byte,
                self.cfg.max_size,
                self.counter.as_ref(),
            );

            let candidates = scan_boundaries(
                &normalized,
                cursor_byte,
                window_end_byte - cursor_byte,
            );

            let chosen = choose_boundary(
                &normalized,
                cursor_byte,
                window_end_byte,
                &candidates,
                self.cfg.min_size,
                self.counter.as_ref(),
            )
            .unwrap_or_else(|| {
                forced_boundary(cursor_byte, window_end_byte - cursor_byte, &normalized)
            });

            let mut end_byte = chosen.end_byte.max(cursor_byte + 1);
            while end_byte < normalized.len() && !normalized.is_char_boundary(end_byte) {
                end_byte += 1;
            }

            let slice = &normalized[cursor_byte..end_byte];
            let char_len = slice.chars().count();
            chunks.push(Chunk {
                index: chunk_index,
                text: slice.to_string(),
                boundary: map_kind(chosen.kind),
                start_byte: cursor_byte,
                end_byte,
                char_len,
            });
            chunk_index += 1;

            if end_byte >= normalized.len() {
                break;
            }

            let next_cursor = if self.cfg.overlap > 0 {
                retreat_for_overlap(
                    &normalized,
                    end_byte,
                    self.cfg.overlap,
                    self.counter.as_ref(),
                )
            } else {
                let mut n = end_byte;
                while n < normalized.len() {
                    let c = normalized[n..].chars().next().unwrap();
                    if !c.is_whitespace() {
                        break;
                    }
                    n += c.len_utf8();
                }
                n
            };

            if next_cursor <= cursor_byte {
                cursor_byte += normalized[cursor_byte..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
            } else {
                cursor_byte = next_cursor;
            }
        }

        tracing::debug!(
            event.name = "ragloom.chunker.recursive.done",
            chunks_produced = chunks.len(),
            "ragloom.chunker.recursive.done",
        );

        Ok(ChunkedDocument { chunks })
    }

    fn strategy_fingerprint(&self) -> &StrategyFingerprint {
        &self.fingerprint
    }
}

fn advance_to_budget(
    text: &str,
    start: usize,
    max_size: usize,
    counter: &dyn TokenCounter,
) -> usize {
    let mut probe = (start + max_size * 4).min(text.len());
    while probe > start && !text.is_char_boundary(probe) {
        probe -= 1;
    }
    if counter.count(&text[start..probe]) <= max_size {
        return probe;
    }

    let mut lo = start;
    let mut hi = probe;
    while lo + 1 < hi {
        let mid_raw = lo + (hi - lo) / 2;
        let mut mid = mid_raw;
        while mid > lo && !text.is_char_boundary(mid) {
            mid -= 1;
        }
        if mid == lo {
            // Cannot probe strictly between lo and hi on a char boundary —
            // move hi down to the next char boundary above lo to make progress.
            let mut up = lo + 1;
            while up < hi && !text.is_char_boundary(up) {
                up += 1;
            }
            if up >= hi {
                break;
            }
            if counter.count(&text[start..up]) <= max_size {
                lo = up;
            } else {
                hi = up;
            }
            continue;
        }
        if counter.count(&text[start..mid]) <= max_size {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

fn choose_boundary(
    text: &str,
    start: usize,
    window_end: usize,
    candidates: &[Boundary],
    min_size: usize,
    counter: &dyn TokenCounter,
) -> Option<Boundary> {
    use EngBoundaryKind::*;
    for target in [Paragraph, Line, Sentence, Whitespace] {
        let chosen = candidates
            .iter()
            .filter(|b| b.kind == target && b.end_byte > start && b.end_byte <= window_end)
            .filter(|b| counter.count(&text[start..b.end_byte]) >= min_size)
            .max_by_key(|b| b.end_byte);
        if let Some(&b) = chosen {
            return Some(b);
        }
    }
    None
}

fn retreat_for_overlap(
    text: &str,
    end: usize,
    overlap: usize,
    counter: &dyn TokenCounter,
) -> usize {
    let mut lo = 0usize;
    let mut hi = end;
    while lo + 1 < hi {
        let mid_raw = lo + (hi - lo) / 2;
        let mut mid = mid_raw;
        while mid > lo && !text.is_char_boundary(mid) {
            mid -= 1;
        }
        if mid == lo {
            let mut up = lo + 1;
            while up < hi && !text.is_char_boundary(up) {
                up += 1;
            }
            if up >= hi {
                break;
            }
            if counter.count(&text[up..end]) <= overlap {
                hi = up;
            } else {
                lo = up;
            }
            continue;
        }
        if counter.count(&text[mid..end]) <= overlap {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars_cfg(max: usize) -> RecursiveConfig {
        RecursiveConfig {
            metric: SizeMetric::Chars,
            max_size: max,
            min_size: 0,
            overlap: 0,
        }
    }

    #[test]
    fn empty_input_produces_no_chunks() {
        let c = RecursiveChunker::new(chars_cfg(10)).unwrap();
        assert!(c.chunk("").unwrap().chunks.is_empty());
    }

    #[test]
    fn zero_budget_produces_no_chunks() {
        let c = RecursiveChunker::new(chars_cfg(0)).unwrap();
        assert!(c.chunk("hello").unwrap().chunks.is_empty());
    }

    #[test]
    fn fixed_size_split_for_ascii() {
        let c = RecursiveChunker::new(chars_cfg(4)).unwrap();
        let doc = c.chunk("abcdefghij").unwrap();
        let texts: Vec<&str> = doc.chunks.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn multibyte_unicode_does_not_panic() {
        let c = RecursiveChunker::new(chars_cfg(3)).unwrap();
        let doc = c.chunk("你好世界🙂hello").unwrap();
        assert!(!doc.chunks.is_empty());
        for ch in &doc.chunks {
            assert!(!ch.text.is_empty());
            assert!(ch.text.is_char_boundary(0));
        }
    }

    #[test]
    fn boundary_preference_prefers_paragraph_over_line() {
        let cfg = RecursiveConfig { max_size: 8, min_size: 0, overlap: 0, metric: SizeMetric::Chars };
        let c = RecursiveChunker::new(cfg).unwrap();
        let doc = c.chunk("aaaa\n\nbbbb cccc\ndddd").unwrap();
        assert!(doc.chunks.len() >= 2);
        assert_eq!(doc.chunks[0].text, "aaaa");
        assert_eq!(doc.chunks[0].boundary, super::super::BoundaryKind::Paragraph);
    }

    #[test]
    fn overlap_retreats_cursor() {
        let cfg = RecursiveConfig { max_size: 5, min_size: 0, overlap: 2, metric: SizeMetric::Chars };
        let c = RecursiveChunker::new(cfg).unwrap();
        let doc = c.chunk("abcdefghij").unwrap();
        let texts: Vec<&str> = doc.chunks.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["abcde", "defgh", "ghij"]);
    }

    #[test]
    fn fingerprint_is_stable_and_contains_parameters() {
        let c = RecursiveChunker::new(chars_cfg(123)).unwrap();
        let fp = c.strategy_fingerprint().as_str();
        assert!(fp.starts_with("recursive:v1|metric=chars"));
        assert!(fp.contains("max=123"));
    }

    #[test]
    fn invalid_config_is_rejected() {
        let bad = RecursiveConfig { metric: SizeMetric::Chars, max_size: 10, min_size: 11, overlap: 0 };
        assert!(RecursiveChunker::new(bad).is_err());
        let bad2 = RecursiveConfig { metric: SizeMetric::Chars, max_size: 10, min_size: 0, overlap: 10 };
        assert!(RecursiveChunker::new(bad2).is_err());
    }
}
