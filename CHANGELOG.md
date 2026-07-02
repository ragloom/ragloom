# Changelog

## [Unreleased]

### Changed
- Established Rust 1.88 as the MSRV, declared it in both workspace package
  manifests, and added a dedicated all-targets, all-features MSRV CI check.
  Patch releases will not raise the MSRV.

### Removed
- Removed the deprecated `ChunkerConfig`, `ChunkingStrategy`,
  `chunk_document`, and `chunk_text` compatibility APIs. Use
  `RecursiveChunker` through the `Chunker` trait instead.

### Security
- Accepted temporary, exact advisory exceptions for `RUSTSEC-2026-0194` and
  `RUSTSEC-2026-0195`. The affected `quick-xml` versions remain transitive
  through `docx-lite 0.2.0` and `rust-s3 0.37.2`, whose released dependency
  constraints do not yet permit the patched `quick-xml 0.41.0`. Crafted DOCX
  XML or responses from a configured S3 endpoint can cause CPU or memory
  exhaustion; remove both exceptions as soon as both upstream crates permit
  the patched version.

## [0.5.0] - 2026-07-02

### Added
- Checked-in `v0.4.0` and `v0.4.1` state fixtures plus compatibility contract
  tests that open, replay, compact, and re-open supported `wal.ndjson` and
  `failed.ndjson` shapes under the current implementation.

### Changed
- Upgraded `pdf-extract` to `0.12.0` and `lopdf` to `0.42.0` to address
  `RUSTSEC-2026-0187`. The no-fix `ttf-parser` maintenance advisory remains a
  narrowly scoped `cargo-deny` exception until `lopdf` migrates away from it.
- Updated `quinn-proto` to `0.11.15` to address `RUSTSEC-2026-0185`.
- Accepted the narrowly scoped `RUSTSEC-2024-0436` maintenance risk for
  `paste 1.0.15`, which remains only in the opt-in `fastembed` dependency
  graph while the upstream `tokenizers` migration is unresolved. The default
  embedding path does not include this dependency, and removal remains tracked
  in issue #67.
- Defined the `v0.5.0` on-disk compatibility contract as direct readability of
  supported released `v0.4.x` WAL state, with `v0.4.0` as the minimum
  supported WAL version and `failed.ndjson` support applying from `v0.4.1+`.
- Kept state compatibility fail-closed: unknown future records, malformed
  lines, truncated final writes, and unsupported state shapes now have
  explicit contract coverage and remain startup errors instead of best-effort
  recovery cases.
- Defined the v0.5 compatibility boundary for deterministic point identity,
  Qdrant payloads, CLI defaults, and durable state.
  The Qdrant point ID is the chunk identity.
  It is not duplicated as a `chunk_id` payload field.
  Strategy-fingerprint changes intentionally open a new point-ID space.
  IDs are not reused for different chunk content.
- Marked `canonical_path`, `doc_id`, `tenant_id`, `file_extension`,
  `size_bytes`, `mtime_unix_secs`, `chunk_index`, `total_chunks`,
  `previous_chunk_id`, `next_chunk_id`, `chunk_start_byte`, `chunk_end_byte`,
  `chunk_char_len`, `chunk_text_sha256`, and `strategy_fingerprint` as the
  stable v0.5 payload fields. `chunk_text` is optional compatibility data;
  additive fields remain non-contractual until explicitly promoted.
- Recorded that the default embedding backend remains OpenAI with
  `text-embedding-3-small`, the default chunker mode remains `router`, state
  remains at `.ragloom/wal.ndjson`, health remains disabled, and existing
  sizing and retry defaults remain unchanged. Semantic chunking remains
  experimental and opt-in.

### Docs
- Documented the state upgrade contract consistently across `README.md`,
  `SUPPORT.md`, and `CHANGELOG.md`, including the minimum supported state
  version and the requirement for explicit migration notes before any future
  incompatible state change.
- Added an operator action matrix for the v0.5 compatibility boundary:
  additive payload fields and behavior-preserving fixes do not require
  operator action. Incompatible changes require release-note migration guidance.

## [0.4.1] - 2026-06-02

### Added
- Explicit `ragloom compact-state` command to crash-safely compact
  `wal.ndjson` and `failed.ndjson` without changing startup replay,
  planner de-duplication, delete-sync behavior, or failed-work replay
  semantics.
- Persistent local failed-work journal at `failed.ndjson` beside the WAL so
  exhausted or terminal ingest work can be inspected and operator-replayed
  without changing WAL acknowledgement or startup replay semantics.
- Minimal `replay-failed` CLI command that requeues pending failed work back
  into the WAL using only `--state-path` or `--config`.

### Changed
- Retry exhaustion no longer stops at logs and counters: worker terminal
  failures now persist a sanitized failure class plus the original scheduled
  work record, without recording secrets or full document contents.
- Semantic chunking remains experimental and opt-in for `v0.4.1`, while
  `--semantic-provider` and `--semantic-percentile` now only apply when the
  semantic path is active in router or single-semantic mode.
- GitHub Release publication now uploads only the packaged archives plus their
  `.sha256.txt` verification files, and filters out unrelated workflow
  artifacts such as coverage outputs.

### Docs
- Documented the state-compaction safety boundary, including Linux/Unix
  rename-plus-directory-sync behavior and Windows native file replacement.
- Clarified that semantic chunking remains experimental and opt-in, and that
  `fastembed` remains a feature-gated semantic provider for both router and
  single-semantic mode.
- Aligned the installation and support docs with the slimmer release-asset set
  and the `v0.4.1` archive examples.

## [0.4.0] - 2026-05-26

### Added
- Polling S3 source support, including typed source configuration, runtime
  wiring, stable source identity semantics, and deterministic planner/WAL
  integration for one configured bucket and prefix.
- Deterministic PDF embedded-text extraction and deterministic DOCX text
  extraction within the document loading boundary.

### Changed
- Broadened the document loader boundary to support built-in UTF-8 text,
  Markdown, source code, PDF, and DOCX ingestion without changing the
  chunking, embedding, sink, or deterministic point-ID architecture.
- Moved source runtime helpers into the library crate so CLI wiring in
  `src/main.rs` stays thinner and reusable runtime behavior remains testable.

### Fixed
- Pinned the `fastembed` pull-request checkout path in `quality-deep` so
  release-critical PR validation keeps testing the intended merge ref.

### Docs
- Aligned `README.md`, `SUPPORT.md`, and `CHANGELOG.md` around the released
  `v0.4` support matrix, including a concrete polling S3 example plus explicit
  PDF, DOCX, and out-of-scope remote-source limits.

## [0.3.0] - 2026-05-22

### Docs
- Reframed the `README.md` `v0.3` roadmap around stability, workflow
  readiness, and code quality hardening instead of broader document-format
  expansion.
- Documented the `v0.3` release-readiness gate and aligned the stable,
  feature-gated, experimental, and best-effort support boundaries across
  `README.md` and `SUPPORT.md`.

## [0.2.1] - 2026-05-12

### Fixed
- Delete synchronization now survives restarts by rebuilding the source's
  previously observed canonical-path set from the WAL before the first scan.
  Files removed while Ragloom is offline are emitted as durable delete work on
  the next startup instead of being stranded in Qdrant.

### Changed
- OpenAI and generic HTTP embedding errors now include a bounded, sanitized
  preview of upstream non-success response bodies to make provider failures
  easier to diagnose without logging secrets or full payloads.

## [0.2.0] - 2026-05-08

### Added
- Persistent local WAL state via `FileWal`, stored by default at
  `.ragloom/wal.ndjson` and configurable with `--state-path` or `state.path`.
- Startup replay for unacknowledged `WorkItemV2` records, with acknowledged file
  versions seeded into planner de-duplication to avoid re-emitting completed
  work after restart.
- Delete synchronization for previously observed source files: completed scans
  emit explicit delete events, the WAL stores separate delete work/ack records,
  and the Qdrant sink deletes all points matching the document's stable
  `doc_id`.
- Bounded in-process retry queue for transient loader I/O, embedding, and sink
  failures, configurable with `retry.*` YAML keys or `--retry-*` CLI flags.
- Opt-in local health endpoint, configurable with `health.addr` or
  `--health-addr`, returning minimal daemon status and build/version JSON.
- Prometheus-compatible `/metrics` endpoint on the same local health listener,
  exposing ingest progress, retry reliability counters, and queue depth gauges.

### Changed
- `FileWal` now keeps its append handle open across records to avoid repeated
  open/close overhead on the WAL hot path, while still calling
  `sync_data()` per append so crash recovery semantics stay unchanged.

### Notes
- The WAL format is append-only newline-delimited JSON. Corrupt or unreadable
  state files fail startup with a `state` error so operators can inspect or
  replace the file intentionally.
- Delete synchronization only covers files Ragloom has already observed under
  the configured source root. It is idempotent and does not manage whole Qdrant
  collections.
- Retries are deterministic and jitter-free. Configuration and invalid-input
  errors are not retried, and exhausted retries are counted in
  `ragloom.ingest.summary` failures.
- The health endpoint is disabled by default and only exposes readiness,
  version, and build metadata. Startup/bootstrap failures and fatal runtime-loop
  failures report `ready: false`.
- The metrics endpoint is enabled with the health listener and exposes numeric
  counters only, without document contents, secrets, or full local paths.

## [0.1.0] - 2026-05-01

### Added
- Pluggable `Chunker` trait with `RecursiveChunker` (phase 1 of the smart
  chunking roadmap) backed by the `chonkie-inc/chunk` SIMD byte-level engine.
- Token-based sizing via `tiktoken_rs::cl100k_base` with a pluggable
  `TokenCounter` trait.
- `StrategyFingerprint` mixed into blake3 point-ID hashing so future chunker
  upgrades never silently collide with older IDs.
- CLI flags: `--chunker-strategy`, `--size-metric`, `--size-max`, `--size-min`,
  `--size-overlap`, `--tokenizer`.
- Structured tracing events `ragloom.chunker.recursive.*` and a `strategy`
  field on the pipeline `process_file` span.
- Content-aware chunking: `ChunkerRouter` dispatches by extension to
  `MarkdownChunker` (pulldown-cmark) and `CodeChunker` (tree-sitter) across
  Rust / Python / JavaScript / TypeScript / Go / Java / C / C++ / Ruby / Bash.
- `ChunkHint` parameter on the `Chunker` trait; `strategy_fingerprint` moved
  onto `ChunkedDocument`.
- CLI flags: `--chunker-mode router|single`, `--chunker-single <kind>`.
- Structured tracing spans `ragloom.chunker.markdown.*` and
  `ragloom.chunker.code.*` with `lang` field on code spans.

### Deprecated
- `chunk_text`, `chunk_document`, `ChunkerConfig`, `ChunkingStrategy` — these
  legacy symbols remain for backwards compatibility but route through the new
  `RecursiveChunker`. Use the `Chunker` trait directly instead.

### Changed
- Point-ID hash input now includes the chunker strategy fingerprint.
  **Migration:** existing Qdrant collections created with prior ragloom builds
  will retain old points but will not be re-associated with new chunks; drop
  or GC the old collection if you want a clean state.
- **Breaking (library API only)**: `Chunker::chunk` now takes
  `(&str, &ChunkHint)`. `Chunker::strategy_fingerprint` removed — fingerprint
  now lives on `ChunkedDocument`.
- Default binary chunker in `main.rs` is now the Router (`--chunker-mode=router`);
  library callers using `PipelineExecutor::new` keep the bare `RecursiveChunker`.

### Migration

- External callers of `Chunker::chunk(text)` must pass `&ChunkHint::none()`
  (or `ChunkHint::from_path(path)` for content-aware dispatch).
- External callers reading `chunker.strategy_fingerprint()` must read
  `doc.strategy_fingerprint` from the returned document.
- Point-ID spaces for `.md` and source-code files change on first Phase 2
  run; drop or GC old Qdrant collections if you want a clean slate.

### Added (Phase 3)

- `SemanticSignalProvider` sync trait with `EmbeddingProviderAdapter`
  bridging the async `EmbeddingProvider`.
- `SemanticChunker` splits prose at p95 adjacent-sentence cosine-distance
  peaks (default percentile, tunable).
- Optional `fastembed` Cargo feature for local ONNX sentence embeddings.
- `--enable-semantic`, `--semantic-provider`, `--semantic-percentile` CLI flags.

### Migration (Phase 3)

- Enabling `--enable-semantic` moves `.md` / `.txt` documents into a new
  `semantic:v1|…` point-ID space. Phase 2 recursive / markdown points remain
  untouched but will not be re-associated.
- The `Chunker` trait is unchanged; Phase 2 callers are unaffected.
