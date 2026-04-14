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
