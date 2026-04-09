//! Criterion sanity benchmark.
//!
//! # Why
//! We wire Criterion early so later performance work (e.g., chunking and batching)
//! can land with reproducible measurements and without CI/tooling surprises.

use criterion::{Criterion, criterion_group, criterion_main};

fn criterion_sanity_benchmark(c: &mut Criterion) {
    c.bench_function("sanity_add", |b| b.iter(|| 1u64.wrapping_add(1)));
}

criterion_group!(benches, criterion_sanity_benchmark);
criterion_main!(benches);
