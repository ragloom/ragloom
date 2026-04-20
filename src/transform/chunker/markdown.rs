//! Markdown-aware chunker backed by `pulldown-cmark`.
//!
//! # Why
//! Markdown has natural hard boundaries (headings, fenced code blocks,
//! paragraphs) that carry semantic meaning for retrieval. A generic
//! character-level chunker loses those boundaries. The Markdown chunker
//! collects strong offsets from the event stream, then hands the resulting
//! slabs to the shared RecursiveChunker when they exceed the size budget.

use std::sync::Arc;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::error::ChunkResult;
use super::fingerprint::StrategyFingerprint;
use super::recursive::{RecursiveChunker, RecursiveConfig};
use super::size::SizeMetric;
use super::{Chunk, ChunkHint, ChunkedDocument, Chunker};

pub struct MarkdownChunker {
    inner: Arc<RecursiveChunker>,
    fingerprint: StrategyFingerprint,
}

impl MarkdownChunker {
    pub fn new(config: RecursiveConfig) -> ChunkResult<Self> {
        let inner = Arc::new(RecursiveChunker::new(config)?);
        let metric_str = match config.metric {
            SizeMetric::Chars => "chars",
            SizeMetric::Tokens => "tokens",
        };
        let fp = format!(
            "markdown:v1|metric={}|max={}|min={}|overlap={}",
            metric_str, config.max_size, config.min_size, config.overlap
        );
        Ok(Self {
            inner,
            fingerprint: StrategyFingerprint::new(fp),
        })
    }

    pub fn fingerprint(&self) -> &StrategyFingerprint {
        &self.fingerprint
    }

    fn collect_boundaries(&self, text: &str) -> Vec<usize> {
        let mut boundaries: Vec<usize> = vec![0];
        for (event, range) in Parser::new(text).into_offset_iter() {
            match event {
                Event::Start(Tag::Heading { .. }) => {
                    boundaries.push(range.start);
                }
                Event::End(TagEnd::Heading(_))
                | Event::End(TagEnd::Paragraph)
                | Event::End(TagEnd::CodeBlock) => {
                    boundaries.push(range.end);
                }
                _ => {}
            }
        }
        boundaries.push(text.len());
        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries.retain(|&b| text.is_char_boundary(b));
        boundaries
    }
}

impl Chunker for MarkdownChunker {
    #[tracing::instrument(
        name = "ragloom.chunker.markdown.chunk",
        skip(self, text, _hint),
        fields(bytes = text.len(), strategy = %self.fingerprint)
    )]
    fn chunk(&self, text: &str, _hint: &ChunkHint<'_>) -> ChunkResult<ChunkedDocument> {
        if text.is_empty() {
            return Ok(ChunkedDocument {
                chunks: Vec::new(),
                strategy_fingerprint: self.fingerprint.clone(),
            });
        }

        let boundaries = self.collect_boundaries(text);
        let mut chunks: Vec<Chunk> = Vec::new();
        let mut next_index = 0usize;

        for window in boundaries.windows(2) {
            let (start, end) = (window[0], window[1]);
            if start >= end {
                continue;
            }
            let slab = &text[start..end];
            if slab.trim().is_empty() {
                continue;
            }

            let inner_doc = self.inner.chunk(slab, &ChunkHint::none())?;
            for chunk in inner_doc.chunks {
                chunks.push(Chunk {
                    index: next_index,
                    text: chunk.text,
                    boundary: chunk.boundary,
                    start_byte: start + chunk.start_byte,
                    end_byte: start + chunk.end_byte,
                    char_len: chunk.char_len,
                });
                next_index += 1;
            }
        }

        Ok(ChunkedDocument {
            chunks,
            strategy_fingerprint: self.fingerprint.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunker(max: usize) -> MarkdownChunker {
        MarkdownChunker::new(RecursiveConfig {
            metric: SizeMetric::Chars,
            max_size: max,
            min_size: 0,
            overlap: 0,
        })
        .unwrap()
    }

    #[test]
    fn empty_input_produces_no_chunks() {
        let doc = chunker(100).chunk("", &ChunkHint::none()).unwrap();
        assert!(doc.chunks.is_empty());
        assert!(doc.strategy_fingerprint.as_str().starts_with("markdown:v1"));
    }

    #[test]
    fn heading_creates_boundary() {
        let text = "# One\n\nAlpha.\n\n# Two\n\nBeta.\n";
        let doc = chunker(1000).chunk(text, &ChunkHint::none()).unwrap();
        assert!(doc.chunks.len() >= 2, "got chunks: {:?}", doc.chunks);
        assert!(doc.chunks.iter().any(|c| c.text.contains("Alpha.")));
        assert!(doc.chunks.iter().any(|c| c.text.contains("Beta.")));
    }

    #[test]
    fn code_block_stays_intact_when_it_fits_budget() {
        let text = "# T\n\n```rust\nfn main() { println!(\"hi\"); }\n```\n\nAfter.\n";
        let doc = chunker(1000).chunk(text, &ChunkHint::none()).unwrap();
        let has_fence = doc
            .chunks
            .iter()
            .any(|c| c.text.contains("```rust") && c.text.contains("fn main"));
        assert!(has_fence, "code block should be in a single chunk: {:?}", doc.chunks);
    }

    #[test]
    fn fingerprint_encodes_parameters() {
        let fp = chunker(777)
            .chunk("# x\n", &ChunkHint::none())
            .unwrap()
            .strategy_fingerprint;
        assert_eq!(
            fp.as_str(),
            "markdown:v1|metric=chars|max=777|min=0|overlap=0"
        );
    }
}
