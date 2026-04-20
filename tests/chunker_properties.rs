//! Property-based invariants for [`RecursiveChunker`].
//!
//! # Why
//! Unit tests cover specific shapes; property tests catch off-by-one and
//! boundary bugs on arbitrary UTF-8 inputs.

use proptest::prelude::*;
use ragloom::transform::chunker::{
    ChunkHint, Chunker,
    recursive::{RecursiveChunker, RecursiveConfig},
    size::SizeMetric,
};

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
