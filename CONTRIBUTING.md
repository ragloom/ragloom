# Contributing

Issues and pull requests are welcome. Keep changes small, test-backed, and aligned with Ragloom's minimalist design goals.

## Local Verification

Run these checks before opening a pull request:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-targets --all-features`

Deeper checks used by CI on `main` and release paths:

- `cargo llvm-cov --workspace --all-features`
- `cargo test --workspace --features loom`
- `cargo +nightly miri test --workspace`
- `cargo bench`

## Development Expectations

- Follow TDD for behavior changes and bug fixes.
- Keep APIs open for extension and narrow in responsibility.
- Prefer precise, self-explanatory names over abbreviations.
- Preserve Ragloom's custom error model and attach context at the failure site.
- Update documentation when support policy, release behavior, or configuration changes.

## Testing Guidelines

Focus tests on observable behavior rather than implementation details.

### Test areas by module

- **CLI parsing** (`src/main.rs`): required flags, defaults, invalid combinations, feature-gated options
- **Chunking** (`src/transform`): boundaries, offsets, fingerprints, language routing, semantic behavior, deterministic output
- **ID generation** (`src/ids`): stable IDs, collision-sensitive inputs, strategy fingerprint changes
- **Pipeline/runtime** (`src/pipeline`): acknowledgement behavior, worker shutdown, queue behavior, retry/idempotency assumptions
- **Source loading** (`src/source`, `src/doc`): UTF-8 handling, file metadata, path/canonicalization behavior
- **Embedding clients** (`src/embed`): request/response shape, error mapping, timeout/config validation
- **Qdrant sink** (`src/sink`): payload shape, deterministic upsert behavior, error handling
- **Observability** (`src/observability`): environment parsing and tracing format selection

### Test conventions

- Add regression tests for bug fixes that fail before the fix
- Use `cargo test --workspace --all-targets --all-features` for fast feedback
- Run `cargo test --workspace --features loom` for concurrency-sensitive code
- Run `cargo +nightly miri test --workspace` for unsafe or memory-sensitive code
- Use `cargo llvm-cov --workspace --all-features` to check coverage
