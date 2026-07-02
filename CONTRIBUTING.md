# Contributing

Issues and pull requests are welcome. Keep changes small, test-backed, and aligned with Ragloom's minimalist design goals.

## Rust Toolchain

The workspace MSRV is Rust 1.88. Use the latest stable Rust release for normal
development and the full local quality gates. When changing dependencies,
language features, or build configuration, also verify the MSRV:

```bash
cargo +1.88 check --workspace --all-targets --all-features --locked
```

Do not merge a dependency update that requires a newer compiler without also
updating both `package.rust-version` declarations, the MSRV CI job,
`SUPPORT.md`, and the release notes. MSRV increases are not allowed in patch
releases.

## Local Verification

The local verification commands are:

- `cargo qa`: the fast default contributor gate
- `cargo maintainer-qa`: the authoritative deeper maintainer gate for
  release-sensitive and current support-boundary work

`cargo qa` runs:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-targets --all-features`

`cargo maintainer-qa` runs `cargo qa` plus:

- `cargo build --features fastembed`
- `cargo test --workspace --all-targets --features fastembed`
- `cargo test --workspace --features loom`
- `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS=-D warnings`
- `cargo deny check`
- `cargo audit`

GitHub Actions also runs the same deeper stability checks on pull requests.

Additional manual deep checks still used on `main` and release paths:

- `cargo llvm-cov --workspace --all-features`
- `cargo bench`

GitHub-hosted security scanning runs through CodeQL and repository governance workflows rather than Miri.

## Development Expectations

- Follow TDD for behavior changes and bug fixes.
- Keep APIs open for extension and narrow in responsibility.
- Prefer precise, self-explanatory names over abbreviations.
- Preserve Ragloom's custom error model and attach context at the failure site.
- Update documentation when support policy, release behavior, or configuration changes.
- For file-backed config reload work, keep the allowlist explicit, reject unsafe runtime changes atomically, and preserve the last good applied config on reload failure.

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
- Run `cargo +nightly miri test -p ragloom --lib` locally when investigating unsafe or UB-sensitive behavior
- Use `cargo +nightly miri test --workspace` only for deeper manual investigations because it is too slow for the release workflow
- Use `cargo llvm-cov --workspace --all-features` to check coverage
