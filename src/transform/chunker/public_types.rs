//! Canonical (non-deprecated) chunk types.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoundaryKind {
    Paragraph,
    Line,
    Whitespace,
    Forced,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub index: usize,
    pub text: String,
    pub boundary: BoundaryKind,
    pub start_byte: usize,
    pub end_byte: usize,
    pub char_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChunkedDocument {
    pub chunks: Vec<Chunk>,
}
