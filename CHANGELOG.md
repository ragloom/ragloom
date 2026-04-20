# Changelog

## [Unreleased]

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
