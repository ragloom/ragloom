//! Legacy chunker API preserved as deprecated shims.
//!
//! # Why
//! External callers and existing tests still import `chunk_text` /
//! `chunk_document` / `ChunkerConfig`. Phase 1 keeps them working against the
//! same semantics while emitting a deprecation warning. Task 7 will re-wire
//! these shims to route through `RecursiveChunker`; Task 2 keeps the behavior
//! byte-identical to the old `chunker.rs`.

use super::public_types::{BoundaryKind, Chunk, ChunkedDocument};

/// Chunker configuration.
///
/// # Why
/// Keeping parameters explicit makes chunk boundaries reproducible, which is
/// required for deterministic IDs and idempotent sink writes.
#[deprecated(
    since = "0.2.0",
    note = "use RecursiveChunker through the Chunker trait"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkerConfig {
    pub max_chars: usize,
    pub min_chars: usize,
    pub overlap_chars: usize,
    #[allow(deprecated)]
    pub strategy: ChunkingStrategy,
}

#[allow(deprecated)]
impl ChunkerConfig {
    /// Compatibility constructor (simple max-char chunking, no overlap).
    #[deprecated(
        since = "0.2.0",
        note = "use RecursiveChunker through the Chunker trait"
    )]
    pub fn new(max_chars: usize) -> Self {
        Self {
            max_chars,
            min_chars: 0,
            overlap_chars: 0,
            strategy: ChunkingStrategy::BoundaryAware,
        }
    }
}

#[deprecated(
    since = "0.2.0",
    note = "use RecursiveChunker through the Chunker trait"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkingStrategy {
    BoundaryAware,
}

#[deprecated(
    since = "0.2.0",
    note = "use RecursiveChunker through the Chunker trait"
)]
#[allow(deprecated)]
pub fn chunk_document(text: &str, cfg: &ChunkerConfig) -> ChunkedDocument {
    if cfg.max_chars == 0 || text.is_empty() {
        return ChunkedDocument { chunks: Vec::new() };
    }

    // Normalize CRLF and CR to LF for scanning.
    let normalized = normalize_newlines(text);

    // Precompute UTF-8 safe byte boundaries for each char.
    let mut boundaries: Vec<usize> = normalized.char_indices().map(|(i, _)| i).collect();
    boundaries.push(normalized.len());

    let mut chunks: Vec<Chunk> = Vec::new();

    let mut start_char = 0usize;
    let mut chunk_index = 0usize;
    while start_char < boundaries.len().saturating_sub(1) {
        let max_end_char = (start_char + cfg.max_chars).min(boundaries.len() - 1);

        let split = find_split_point(&normalized, &boundaries, start_char, max_end_char, cfg);
        let mut end_char = split.end_char;

        if end_char <= start_char {
            // Ensure progress and avoid emitting empty chunks.
            end_char = (start_char + 1).min(boundaries.len() - 1);
        }

        let start_byte = boundaries[start_char];
        let end_byte = boundaries[end_char];
        let slice = &normalized[start_byte..end_byte];

        let char_len = slice.chars().count();

        chunks.push(Chunk {
            index: chunk_index,
            text: slice.to_string(),
            boundary: split.boundary,
            start_byte,
            end_byte,
            char_len,
        });
        chunk_index += 1;

        if end_char >= boundaries.len() - 1 {
            break;
        }

        // Compute next start with overlap (char-count based).
        let mut next_start = end_char;
        if cfg.overlap_chars > 0 {
            next_start = next_start.saturating_sub(cfg.overlap_chars);
        } else {
            // Do not start a new chunk on boundary-only characters.
            //
            // # Why
            // When splitting on paragraph/line boundaries, we want the separator to
            // act as a delimiter, not as content. Skipping leading whitespace/newlines
            // keeps chunks semantically meaningful and avoids empty chunks.
            while next_start < boundaries.len() - 1 {
                let b = boundaries[next_start];
                if normalized[b..].chars().next().map(|c| c.is_whitespace()) != Some(true) {
                    break;
                }
                next_start += 1;
            }
        }
        if next_start <= start_char {
            // Ensure progress even if overlap >= produced chunk size.
            next_start = start_char + 1;
        }
        start_char = next_start;
    }

    ChunkedDocument { chunks }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SplitPoint {
    end_char: usize,
    boundary: BoundaryKind,
}

fn normalize_newlines(text: &str) -> String {
    // Two-pass, deterministic normalization: CRLF -> LF, then CR -> LF.
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if let Some('\n') = chars.peek().copied() {
                    chars.next();
                }
                out.push('\n');
            }
            _ => out.push(ch),
        }
    }
    out
}

#[allow(deprecated)]
fn find_split_point(
    text: &str,
    boundaries: &[usize],
    start_char: usize,
    max_end_char: usize,
    cfg: &ChunkerConfig,
) -> SplitPoint {
    let start_byte = boundaries[start_char];

    // We never scan past `max_end_char`.
    //
    // # Why
    // `max_chars` is a hard budget. `min_chars` is a soft preference: we prefer
    // boundaries *after* the minimum when possible, but we must not exceed the
    // maximum budget.
    let window_end_char = max_end_char.min(boundaries.len() - 1);
    let window_end_byte = boundaries[window_end_char];

    let min_end_char = if cfg.min_chars > 0 {
        (start_char + cfg.min_chars).min(window_end_char)
    } else {
        start_char + 1
    };

    // Prefer boundaries in [min_end_char, window_end_char].

    // Scan bytes once, tracking boundary positions in char indices.
    let mut last_para: Option<usize> = None;
    let mut last_line: Option<usize> = None;
    let mut last_ws: Option<usize> = None;

    // Track char positions while iterating.
    for (rel_byte, ch) in text[start_byte..window_end_byte].char_indices() {
        let abs_byte = start_byte + rel_byte;
        let abs_char = byte_to_char_index(boundaries, abs_byte).unwrap_or(start_char);

        match ch {
            '\n' => {
                // Line break boundary after this newline.
                let after = abs_char + 1;
                if after > start_char {
                    last_line = Some(after);
                }

                // Paragraph break: two consecutive newlines.
                //
                // # Why
                // We treat "\n\n" as a stronger boundary than a single line break, but we
                // must only classify it as a paragraph break when the full separator is
                // within the scan window. We also advance past the separator so it does
                // not leak into the next chunk.
                if abs_char < window_end_char {
                    let next_byte = boundaries[abs_char + 1];
                    if next_byte < window_end_byte
                        && text[next_byte..window_end_byte].starts_with('\n')
                    {
                        // Split before the paragraph separator so it does not
                        // appear in either chunk.
                        if abs_char > start_char {
                            last_para = Some(abs_char);
                        }
                    }
                }
            }
            c if c.is_whitespace() => {
                // Whitespace boundary after this char (excluding newlines handled above).
                let after = abs_char + 1;
                if after > start_char {
                    last_ws = Some(after);
                }
            }
            _ => {}
        }
    }

    // Choose the best boundary within [min_end_char, window_end_char], then fall back to forced.
    if let Some(end) = last_para.filter(|&e| e >= min_end_char && e <= window_end_char) {
        return SplitPoint {
            end_char: end,
            boundary: BoundaryKind::Paragraph,
        };
    }
    if let Some(end) = last_line.filter(|&e| e >= min_end_char && e <= window_end_char) {
        return SplitPoint {
            end_char: end,
            boundary: BoundaryKind::Line,
        };
    }
    if let Some(end) = last_ws.filter(|&e| e >= min_end_char && e <= window_end_char) {
        return SplitPoint {
            end_char: end,
            boundary: BoundaryKind::Whitespace,
        };
    }

    // Forced split at window_end_char.
    SplitPoint {
        end_char: window_end_char,
        boundary: BoundaryKind::Forced,
    }
}

fn byte_to_char_index(boundaries: &[usize], byte_idx: usize) -> Option<usize> {
    // boundaries is sorted. Use binary search for exact match.
    boundaries.binary_search(&byte_idx).ok()
}

/// Splits text into chunks using a simple character budget.
///
/// # Why
/// This MVP chunker is intentionally simple and deterministic. More advanced
/// strategies (token-based, sentence-aware) can be introduced later without
/// modifying call sites.
#[deprecated(
    since = "0.2.0",
    note = "use RecursiveChunker through the Chunker trait"
)]
#[allow(deprecated)]
pub fn chunk_text(text: &str, cfg: ChunkerConfig) -> Vec<String> {
    // Compatibility adapter: keep the old API but route through chunk_document.
    chunk_document(text, &cfg)
        .chunks
        .into_iter()
        .map(|c| c.text)
        .collect()
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_produces_no_chunks() {
        let chunks = chunk_text("", ChunkerConfig::new(10));
        assert!(chunks.is_empty());
    }

    #[test]
    fn zero_budget_produces_no_chunks() {
        let chunks = chunk_text("hello", ChunkerConfig::new(0));
        assert!(chunks.is_empty());
    }

    #[test]
    fn splits_into_fixed_size_chunks() {
        let chunks = chunk_text("abcdefghij", ChunkerConfig::new(4));
        assert_eq!(chunks, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn chunk_text_handles_multibyte_unicode_without_panicking() {
        let text = "你好世界🙂hello";
        let chunks = chunk_text(text, ChunkerConfig::new(3));
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(!c.is_empty());
            let _ = c.chars().count();
        }
    }

    #[test]
    fn boundary_preference_prefers_paragraph_then_line_then_whitespace() {
        let cfg = ChunkerConfig {
            max_chars: 8,
            min_chars: 0,
            overlap_chars: 0,
            strategy: ChunkingStrategy::BoundaryAware,
        };

        // The first chunk should cut at the paragraph break (\n\n) even though
        // there are other possible boundaries earlier.
        let text = "aaaa\n\nbbbb cccc\ndddd";
        let doc = chunk_document(text, &cfg);
        assert!(doc.chunks.len() >= 2);
        assert_eq!(doc.chunks[0].text, "aaaa");
        assert_eq!(doc.chunks[0].boundary, BoundaryKind::Paragraph);

        // Paragraph separator must not leak into the next chunk.
        assert_eq!(doc.chunks[1].text, "bbbb ");

        // Ensure we didn't create a boundary-only chunk.
        assert_ne!(doc.chunks[1].boundary, BoundaryKind::Paragraph);
    }

    #[test]
    fn overlap_moves_next_start_backward_by_chars() {
        let cfg = ChunkerConfig {
            max_chars: 5,
            min_chars: 0,
            overlap_chars: 2,
            strategy: ChunkingStrategy::BoundaryAware,
        };

        let text = "abcdefghij";
        let doc = chunk_document(text, &cfg);
        let texts: Vec<&str> = doc.chunks.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["abcde", "defgh", "ghij"]);
    }
}
