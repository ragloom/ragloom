//! Chunker benchmark.
//!
//! # Why
//! Chunking can become a throughput bottleneck in large ingestions. We keep a
//! dedicated benchmark to track regressions as strategies evolve.

use criterion::{Criterion, criterion_group, criterion_main};

use ragloom::transform::chunker::{ChunkerConfig, ChunkingStrategy, chunk_document, chunk_text};

fn chunker_benchmark(c: &mut Criterion) {
    let cfg = ChunkerConfig {
        max_chars: 512,
        min_chars: 128,
        overlap_chars: 64,
        strategy: ChunkingStrategy::BoundaryAware,
    };

    let ascii_64kb = "a".repeat(64 * 1024);
    let mixed_unicode_64kb = "你好🙂résumé—".repeat((64 * 1024) / "你好🙂résumé—".len());
    let long_line_64kb = "a".repeat(64 * 1024); // single long line (forced/whitespace)

    c.bench_function("boundary_aware_ascii_64kb", |b| {
        b.iter(|| {
            let doc = chunk_document(&ascii_64kb, &cfg);
            criterion::black_box(doc)
        })
    });

    c.bench_function("boundary_aware_mixed_unicode_64kb", |b| {
        b.iter(|| {
            let doc = chunk_document(&mixed_unicode_64kb, &cfg);
            criterion::black_box(doc)
        })
    });

    c.bench_function("boundary_aware_long_line_64kb", |b| {
        b.iter(|| {
            let doc = chunk_document(&long_line_64kb, &cfg);
            criterion::black_box(doc)
        })
    });

    // Keep legacy adapter benchmark for regressions.
    c.bench_function("chunk_text_64kb_512", |b| {
        b.iter(|| {
            let chunks = chunk_text(&ascii_64kb, cfg);
            criterion::black_box(chunks)
        })
    });
}

criterion_group!(benches, chunker_benchmark);
criterion_main!(benches);
