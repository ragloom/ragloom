//! Chunker throughput benchmark — legacy shim vs direct RecursiveChunker,
//! plus Markdown and Rust CodeChunker sequences.
//!
//! # Why
//! Phase 1 preserves the legacy API through a deprecated shim. This bench
//! confirms the shim does not introduce a meaningful overhead relative to a
//! direct `Chunker` trait call, and measures throughput across a range of
//! document sizes for future regression tracking. Phase 2 adds Markdown and
//! Rust code-aware chunkers to the comparison set.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ragloom::transform::chunker::semantic::{
    SemanticChunker, SemanticSignalProvider, signal::SemanticError,
};
use ragloom::transform::chunker::{
    ChunkHint, Chunker, CodeChunker, MarkdownChunker,
    code::Language,
    recursive::{RecursiveChunker, RecursiveConfig},
    size::SizeMetric,
};
#[allow(deprecated)]
use ragloom::transform::chunker::{ChunkerConfig, chunk_document};

struct StaticSignal;
impl SemanticSignalProvider for StaticSignal {
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, SemanticError> {
        Ok(inputs.iter().map(|_| vec![1.0_f32, 0.0]).collect())
    }
    fn fingerprint(&self) -> &str {
        "bench:static"
    }
}

fn sample(size: usize) -> String {
    let base = "The quick brown fox jumps over the lazy dog. ";
    let mut s = String::with_capacity(size);
    let mut i = 0usize;
    while s.len() < size {
        s.push_str(base);
        i += 1;
        if i.is_multiple_of(5) {
            s.push_str("\n\n");
        }
    }
    s.truncate(size);
    s
}

fn make_md_sample(size: usize) -> String {
    let base = "## Heading\n\nSome body text that forms a paragraph. ";
    let mut s = String::with_capacity(size);
    while s.len() < size {
        s.push_str(base);
    }
    s.truncate(size);
    s
}

fn make_rs_sample(size: usize) -> String {
    let base = "fn task() -> i32 { let x = 1; x + 2 }\n";
    let mut s = String::with_capacity(size);
    while s.len() < size {
        s.push_str(base);
    }
    s.truncate(size);
    s
}

fn bench(c: &mut Criterion) {
    let sizes = [4 * 1024usize, 64 * 1024, 512 * 1024, 2 * 1024 * 1024];
    let mut group = c.benchmark_group("chunker");
    for &n in &sizes {
        let text = sample(n);
        group.throughput(Throughput::Bytes(text.len() as u64));

        group.bench_with_input(BenchmarkId::new("legacy_shim", n), &text, |b, text| {
            #[allow(deprecated)]
            let cfg = ChunkerConfig::new(512);
            b.iter(|| {
                #[allow(deprecated)]
                let _ = chunk_document(text, &cfg);
            });
        });

        group.bench_with_input(
            BenchmarkId::new("recursive_chars_512", n),
            &text,
            |b, text| {
                let chk = RecursiveChunker::new(RecursiveConfig {
                    metric: SizeMetric::Chars,
                    max_size: 512,
                    min_size: 0,
                    overlap: 0,
                })
                .unwrap();
                b.iter(|| chk.chunk(text, &ChunkHint::none()).unwrap());
            },
        );

        let md_sample = make_md_sample(n);
        group.bench_with_input(
            BenchmarkId::new("markdown_chars_512", n),
            &md_sample,
            |b, text| {
                let chk = MarkdownChunker::new(RecursiveConfig {
                    metric: SizeMetric::Chars,
                    max_size: 512,
                    min_size: 0,
                    overlap: 0,
                })
                .unwrap();
                b.iter(|| chk.chunk(text, &ChunkHint::none()).unwrap());
            },
        );

        let rs_sample = make_rs_sample(n);
        group.bench_with_input(
            BenchmarkId::new("code_rust_chars_512", n),
            &rs_sample,
            |b, text| {
                let chk = CodeChunker::new(
                    Language::Rust,
                    RecursiveConfig {
                        metric: SizeMetric::Chars,
                        max_size: 512,
                        min_size: 0,
                        overlap: 0,
                    },
                )
                .unwrap();
                b.iter(|| chk.chunk(text, &ChunkHint::none()).unwrap());
            },
        );

        let semantic_sample = sample(n);
        group.bench_with_input(
            BenchmarkId::new("semantic_static_512", n),
            &semantic_sample,
            |b, text| {
                let chk = SemanticChunker::new(
                    std::sync::Arc::new(StaticSignal),
                    RecursiveConfig {
                        metric: SizeMetric::Chars,
                        max_size: 512,
                        min_size: 0,
                        overlap: 0,
                    },
                    95,
                )
                .unwrap();
                b.iter(|| chk.chunk(text, &ChunkHint::none()).unwrap());
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
