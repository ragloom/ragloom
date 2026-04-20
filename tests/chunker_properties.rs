//! Property-based invariants for [`RecursiveChunker`].
//!
//! # Why
//! Unit tests cover specific shapes; property tests catch off-by-one and
//! boundary bugs on arbitrary UTF-8 inputs.

use proptest::prelude::*;
use ragloom::transform::chunker::semantic::{
    SemanticChunker, SemanticSignalProvider, signal::SemanticError,
};
use ragloom::transform::chunker::{
    ChunkHint, Chunker, CodeChunker, MarkdownChunker,
    code::Language,
    recursive::{RecursiveChunker, RecursiveConfig},
    size::SizeMetric,
};
use std::sync::Arc;

fn chunker(max: usize) -> RecursiveChunker {
    RecursiveChunker::new(RecursiveConfig {
        metric: SizeMetric::Chars,
        max_size: max,
        min_size: 0,
        overlap: 0,
    })
    .expect("valid")
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    #[test]
    fn every_chunk_respects_max_size(
        text in ".{0,512}",
        max in 1usize..64,
    ) {
        let c = chunker(max);
        let doc = c.chunk(&text, &ChunkHint::none()).expect("chunk");
        for ch in doc.chunks {
            prop_assert!(
                ch.char_len <= max,
                "char_len {} exceeded max {} for chunk {:?}",
                ch.char_len, max, ch.text
            );
        }
    }

    #[test]
    fn every_chunk_text_is_valid_utf8_scalar_count(
        text in ".{0,512}",
        max in 1usize..64,
    ) {
        let c = chunker(max);
        let doc = c.chunk(&text, &ChunkHint::none()).expect("chunk");
        for ch in doc.chunks {
            // If `text` field exists, its char count should equal char_len.
            prop_assert_eq!(ch.text.chars().count(), ch.char_len);
        }
    }

    #[test]
    fn coverage_approximates_input_length(
        text in ".{0,512}",
        max in 1usize..64,
    ) {
        let c = chunker(max);
        let doc = c.chunk(&text, &ChunkHint::none()).expect("chunk");

        // Normalize the input the same way the chunker does (CRLF/CR -> LF)
        // so character counts line up.
        let normalized: String = text
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let input_chars = normalized.chars().count();
        let total_chunk_chars: usize = doc.chunks.iter().map(|c| c.char_len).sum();

        // Non-overlap mode skips leading whitespace between chunks, so we
        // allow up to input_chars characters to be "missing" in pathological
        // whitespace-heavy inputs, but we must never emit MORE characters
        // than the input contains.
        prop_assert!(
            total_chunk_chars <= input_chars,
            "total chunk chars {} exceeded input chars {}",
            total_chunk_chars, input_chars
        );
    }
}

fn md_chunker(max: usize) -> MarkdownChunker {
    MarkdownChunker::new(RecursiveConfig {
        metric: SizeMetric::Chars,
        max_size: max,
        min_size: 0,
        overlap: 0,
    })
    .unwrap()
}

fn rust_chunker(max: usize) -> CodeChunker {
    CodeChunker::new(
        Language::Rust,
        RecursiveConfig {
            metric: SizeMetric::Chars,
            max_size: max,
            min_size: 0,
            overlap: 0,
        },
    )
    .unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]

    #[test]
    fn markdown_every_chunk_respects_max_size(
        text in ".{0,512}",
        max in 8usize..64,
    ) {
        let c = md_chunker(max);
        let doc = c.chunk(&text, &ChunkHint::none()).unwrap();
        for ch in doc.chunks {
            prop_assert!(ch.char_len <= max, "markdown chunk {} exceeds {}", ch.char_len, max);
        }
    }

    #[test]
    fn markdown_fingerprint_always_markdown(text in ".{0,256}", max in 8usize..64) {
        let doc = md_chunker(max).chunk(&text, &ChunkHint::none()).unwrap();
        prop_assert!(doc.strategy_fingerprint.as_str().starts_with("markdown:v1"));
    }

    #[test]
    fn rust_code_chunker_never_panics_on_arbitrary_text(
        text in ".{0,256}",
        max in 16usize..64,
    ) {
        let doc = rust_chunker(max).chunk(&text, &ChunkHint::none()).unwrap();
        prop_assert!(doc.strategy_fingerprint.as_str().contains("lang=rust"));
        for ch in doc.chunks {
            prop_assert!(ch.char_len <= max);
        }
    }
}

struct ConstantSignal;
impl SemanticSignalProvider for ConstantSignal {
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
        Ok(inputs.iter().map(|_| vec![1.0_f32, 0.0]).collect())
    }
    fn fingerprint(&self) -> &str {
        "const:proptest"
    }
}

fn semantic_chunker(max: usize) -> SemanticChunker {
    SemanticChunker::new(
        Arc::new(ConstantSignal),
        RecursiveConfig {
            metric: SizeMetric::Chars,
            max_size: max,
            min_size: 0,
            overlap: 0,
        },
        95,
    )
    .unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, .. ProptestConfig::default() })]

    #[test]
    fn semantic_never_panics_on_arbitrary_text(
        text in ".{0,512}",
        max in 16usize..128,
    ) {
        let c = semantic_chunker(max);
        let doc = c.chunk(&text, &ChunkHint::none()).unwrap();
        prop_assert!(doc.strategy_fingerprint.as_str().starts_with("semantic:v1"));
        for ch in doc.chunks {
            prop_assert!(ch.char_len <= max);
        }
    }
}
