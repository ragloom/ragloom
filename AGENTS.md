# AGENTS.md

Guidance for Codex and other AI coding agents working in this repository.

## Project overview

Ragloom is a minimalist RAG ingestion engine written in Rust 2024. It scans local files, detects changed file versions, loads UTF-8 text, chunks documents, sends chunks to an embedding backend, and upserts deterministic points into Qdrant.

The project is intentionally small and explicit. **Preserve the minimalist design**: prefer narrow abstractions, deterministic behavior, small changes, and test-backed implementations.

---

## Repository layout

- `src/lib.rs` exposes the reusable library crate.
- `src/main.rs` is the thin CLI runner and should mostly perform configuration parsing and runtime wiring.
- `src/config` contains typed configuration support.
- `src/doc` contains document loading abstractions and filesystem UTF-8 loading.
- `src/embed` contains embedding provider traits and OpenAI / generic HTTP clients.
- `src/ids` contains deterministic ID generation.
- `src/observability` contains tracing subscriber configuration.
- `src/pipeline` contains planning, runtime, worker execution, and acknowledgements.
- `src/sink` contains vector sink abstractions and Qdrant integration.
- `src/source` contains file discovery sources.
- `src/state` contains in-memory WAL record types.
- `src/transform` contains chunking, chunk metadata, router, markdown/code/semantic chunkers.
- `xtask` contains local developer automation, currently `cargo qa`.

## Build and verification commands

Use these commands from the repository root.

### Fast checks

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
```

### Preferred local gate

```bash
cargo qa
```

`cargo qa` runs formatting, Clippy with warnings denied, and the full workspace test suite.

### Additional deeper checks

Run these when touching concurrency, unsafe-sensitive code, chunking behavior, benchmarking paths, or release-critical code:

```bash
cargo test --workspace --features loom
cargo +nightly miri test --workspace
cargo llvm-cov --workspace --all-features
cargo bench
```

### Feature-specific checks

Semantic chunking with local fastembed support is behind the `fastembed` feature:

```bash
cargo build --features fastembed
cargo test --workspace --all-targets --features fastembed
```

Loom-specific tests are behind the `loom` feature:

```bash
cargo test --workspace --features loom
```

## Coding standards

- Keep `src/main.rs` thin. Put reusable behavior, domain logic, and testable units in the library crate.
- Keep APIs open for extension but narrow in responsibility.
- Prefer precise, self-explanatory names over abbreviations.
- Preserve Ragloom's custom error model.
- Attach error context at the failure site.
- Prefer deterministic behavior for IDs, chunk fingerprints, retries, and sink writes.
- Avoid silently changing point-ID semantics. If chunking or embedding identity changes, ensure fingerprints clearly open a new ID space.
- Keep changes small and focused.
- Update documentation when changing CLI flags, configuration behavior, release behavior, support policy, observability, chunker semantics, or external integration behavior.
- Do not add new runtime dependencies casually. Justify new dependencies in code review.

## Testing expectations

Follow TDD for behavior changes and bug fixes.

When changing code, add or update tests near the affected module. Prefer tests that verify observable behavior rather than implementation details.

Recommended test focus by area:

- CLI parsing in `src/main.rs`: required flags, defaults, invalid combinations, feature-gated options.
- Chunking in `src/transform`: boundaries, offsets, fingerprints, language routing, semantic behavior, deterministic output.
- ID generation in `src/ids`: stable IDs, collision-sensitive inputs, strategy fingerprint changes.
- Pipeline/runtime in `src/pipeline`: acknowledgement behavior, worker shutdown, queue behavior, retry/idempotency assumptions.
- Source loading in `src/source` and `src/doc`: UTF-8 handling, file metadata, path/canonicalization behavior.
- Embedding clients in `src/embed`: request/response shape, error mapping, timeout/config validation.
- Qdrant sink in `src/sink`: payload shape, deterministic upsert behavior, error handling.
- Observability in `src/observability`: environment parsing and tracing format selection.

For bug fixes, add a regression test that fails before the fix.

## CLI behavior

The CLI currently supports both `--flag value` and `--flag=value` forms.

Required runtime flags:

```bash
--dir <path>
--qdrant-url <url>
--collection <name>
```

The default embedding backend is OpenAI and requires:

```bash
--openai-api-key <key>
```

Generic HTTP embedding requires:

```bash
--embed-backend http
--embed-url <url>
```

Do not break documented defaults unless the README and tests are updated together.

## Architecture constraints

The current runtime shape is:

```text
DirectoryScannerSource
  -> Planner
  -> WAL work items
  -> AsyncRuntime queue
  -> PipelineExecutor
     -> DocumentLoader
     -> Chunker
     -> EmbeddingProvider
     -> Sink
  -> Sink acknowledgement
```

Respect the separation between source, document loading, transform/chunking, embedding, sink, and runtime orchestration.

When adding a new backend or behavior:

- Add or reuse a trait in the appropriate module.
- Keep I/O boundaries explicit.
- Keep runtime wiring in `src/main.rs`.
- Add tests for configuration validation and trait-level behavior.
- Update README examples if users need new flags or setup steps.

## Chunking and ID stability

Chunker strategy, sizing parameters, router behavior, and semantic options affect deterministic point IDs.

Be especially careful when modifying:

- `RecursiveChunker`
- `MarkdownChunker`
- `CodeChunker`
- `SemanticChunker`
- `ChunkerRouter`
- chunk metadata
- strategy fingerprint construction
- point ID generation

If output chunk boundaries or fingerprints change, tests should make that explicit.

Never make a chunking change that silently reuses old point IDs for different chunk content.

## Error handling

Use `RagloomError` and `RagloomErrorKind` for project errors.

When mapping lower-level errors, attach context at the boundary where the operation is attempted. Prefer messages that identify the failing operation rather than generic text.

Good pattern:

```rust
some_operation().map_err(|e| {
    RagloomError::new(RagloomErrorKind::Config, e)
        .with_context("failed to build semantic chunker")
})?;
```

Avoid panics in library/runtime code unless the condition is truly unreachable and already validated.

## Observability

Use `tracing` for runtime events.

Prefer structured fields for important context:

```rust
tracing::info!(
    event.name = "ragloom.some_event",
    collection = %collection,
    "ragloom.some_event"
);
```

Do not log secrets, API keys, embedding input payloads, or full document contents.

## Security and secrets

- Never commit API keys, tokens, Qdrant credentials, local paths containing secrets, or generated model/cache files.
- Do not print `OPENAI_API_KEY` or equivalent credentials.
- Treat document text as user data; avoid logging full content.
- Keep network clients timeout-bound.
- Keep dependency additions minimal and review transitive risk.

## Documentation

Update docs when changing:

- CLI flags or defaults
- embedding backend request/response shapes
- Qdrant payload schema
- chunker behavior or supported file extensions
- semantic chunking behavior
- feature flags
- local verification commands
- limitations or scope

Relevant docs include:

- `README.md`
- `CONTRIBUTING.md`
- `SUPPORT.md`
- `SECURITY.md`

## Pull request expectations

Before opening or updating a PR:

1. Run `cargo qa`.
2. Add or update tests for behavior changes.
3. Update docs for user-visible changes.
4. Keep the diff focused.
5. Explain any intentional compatibility break, especially around chunking, IDs, payloads, or CLI flags.

## Agent-specific instructions

When operating as Codex or another AI coding agent:

* Inspect existing module patterns before introducing new patterns.
* Prefer editing the smallest set of files needed.
* Do not rewrite large modules unless requested.
* Do not introduce broad refactors while fixing a narrow bug.
* Do not add dependencies without a strong reason.
* Do not change public behavior without updating tests and docs.
* Run the most relevant tests for the edited area.
* Run `cargo qa` before finalizing when feasible.
* Mention any commands that could not be run and why.
* If tests fail, report the failure and the likely cause instead of claiming success.

## Current project scope

Ragloom currently focuses on:

- local filesystem input
- polling directory scans
- top-level files in the configured directory
- UTF-8 document loading
- Qdrant as the built-in sink
- OpenAI and generic HTTP embedding backends
- in-memory WAL

Avoid expanding scope accidentally. New operational features such as persistent WAL, recursive scanning, collection management, retry queues, health endpoints, and dead-letter handling should be explicit design changes with tests and docs.

```
