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

### Deprecated
- `chunk_text`, `chunk_document`, `ChunkerConfig`, `ChunkingStrategy` — these
  legacy symbols remain for backwards compatibility but route through the new
  `RecursiveChunker`. Use the `Chunker` trait directly instead.

### Changed
- Point-ID hash input now includes the chunker strategy fingerprint.
  **Migration:** existing Qdrant collections created with prior ragloom builds
  will retain old points but will not be re-associated with new chunks; drop
  or GC the old collection if you want a clean state.
