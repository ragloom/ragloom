//! Code-aware chunker built on tree-sitter.
//!
//! # Why
//! Source code has strong structural boundaries. Splitting in the middle of
//! a function or class destroys the semantic unit an embedding should capture.
//! [`CodeChunker`] parses the file with tree-sitter and emits chunks at
//! declaration-level nodes, falling back to recursive splitting only when
//! a single node exceeds the size budget.

pub mod grammars;
pub mod query;

pub use query::Lang as Language;

use std::sync::Arc;

use tree_sitter::Parser;

use super::error::{ChunkError, ChunkResult};
use super::fingerprint::StrategyFingerprint;
use super::recursive::{RecursiveChunker, RecursiveConfig};
use super::size::SizeMetric;
use super::{Chunk, ChunkHint, ChunkedDocument, Chunker};

pub struct CodeChunker {
    lang: Language,
    inner: Arc<RecursiveChunker>,
    fingerprint: StrategyFingerprint,
}

impl CodeChunker {
    pub fn new(lang: Language, config: RecursiveConfig) -> ChunkResult<Self> {
        let inner = Arc::new(RecursiveChunker::new(config)?);
        let metric_str = match config.metric {
            SizeMetric::Chars => "chars",
            SizeMetric::Tokens => "tokens",
        };
        let fp = format!(
            "code:v1|lang={}|grammar={}|metric={}|max={}|min={}|overlap={}",
            lang.as_fingerprint(),
            lang.grammar_fingerprint(),
            metric_str,
            config.max_size,
            config.min_size,
            config.overlap,
        );
        Ok(Self {
            lang,
            inner,
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
}

impl Chunker for CodeChunker {
    #[tracing::instrument(
        name = "ragloom.chunker.code.chunk",
        skip(self, text, _hint),
        fields(
            bytes = text.len(),
            lang = self.lang.as_fingerprint(),
            strategy = %self.fingerprint,
        )
    )]
    fn chunk(&self, text: &str, _hint: &ChunkHint<'_>) -> ChunkResult<ChunkedDocument> {
        if text.is_empty() {
            return self.finalise(Vec::new());
        }

        let mut parser = Parser::new();
        parser
            .set_language(&grammars::language_for(self.lang))
            .map_err(|e| ChunkError::Tokenizer(format!("tree-sitter set_language: {e}")))?;

        let tree = parser
            .parse(text, None)
            .ok_or_else(|| ChunkError::ParseError {
                lang: self.lang.as_fingerprint().to_string(),
                pos: 0,
                detail: "tree_sitter::Parser::parse returned None".to_string(),
            })?;

        let allowed = self.lang.declaration_node_types();
        let root = tree.root_node();

        // Collect declaration-level ranges at the top level.
        let mut node_ranges: Vec<(usize, usize)> = Vec::new();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if allowed.iter().any(|t| *t == child.kind()) {
                node_ranges.push((child.start_byte(), child.end_byte()));
            }
        }

        // No declarations → fall back to full-text recursive chunking.
        if node_ranges.is_empty() {
            let chunks = self.inner.chunk_raw(text)?;
            return self.finalise(chunks);
        }

        // Cover everything: gaps before/between/after declarations go to
        // their own slabs so nothing is lost (file-level comments, imports,
        // trailing blank lines).
        let mut covered: Vec<(usize, usize)> = Vec::new();
        let mut cursor_byte = 0usize;
        for (start, end) in &node_ranges {
            if *start > cursor_byte {
                covered.push((cursor_byte, *start));
            }
            covered.push((*start, *end));
            cursor_byte = *end;
        }
        if cursor_byte < text.len() {
            covered.push((cursor_byte, text.len()));
        }

        let mut chunks: Vec<Chunk> = Vec::new();
        let mut next_index = 0usize;

        for (start, end) in covered {
            let slab = &text[start..end];
            if slab.trim().is_empty() {
                continue;
            }
            // Delegate to inner RecursiveChunker to enforce budget. It returns
            // a single chunk for slabs that already fit.
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

        self.finalise(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunker(lang: Language, max: usize) -> CodeChunker {
        CodeChunker::new(
            lang,
            RecursiveConfig {
                metric: SizeMetric::Chars,
                max_size: max,
                min_size: 0,
                overlap: 0,
            },
        )
        .unwrap()
    }

    #[test]
    fn empty_input_produces_no_chunks() {
        let c = chunker(Language::Rust, 100);
        let doc = c.chunk("", &ChunkHint::none()).unwrap();
        assert!(doc.chunks.is_empty());
        assert!(doc.strategy_fingerprint.as_str().starts_with("code:v1"));
    }

    #[test]
    #[cfg_attr(miri, ignore = "Miri does not support tree-sitter FFI tests")]
    fn rust_two_functions_produce_at_least_two_chunks() {
        let text = "fn a() { 1; }\n\nfn b() { 2; }\n";
        let c = chunker(Language::Rust, 1000);
        let doc = c.chunk(text, &ChunkHint::none()).unwrap();
        assert!(doc.chunks.len() >= 2, "chunks: {:?}", doc.chunks);
    }

    #[test]
    #[cfg_attr(miri, ignore = "Miri does not support tree-sitter FFI tests")]
    fn fingerprint_contains_language_and_grammar() {
        let c = chunker(Language::Rust, 1000);
        let fp = c
            .chunk("fn main(){}", &ChunkHint::none())
            .unwrap()
            .strategy_fingerprint;
        assert!(fp.as_str().contains("lang=rust"));
        assert!(fp.as_str().contains("grammar=tree-sitter-rust@"));
    }

    #[test]
    #[cfg_attr(miri, ignore = "Miri does not support tree-sitter FFI tests")]
    fn bash_function_parses() {
        let text = "greet() { echo hi; }\n";
        let c = chunker(Language::Bash, 1000);
        let doc = c.chunk(text, &ChunkHint::none()).unwrap();
        assert!(!doc.chunks.is_empty());
    }
}
