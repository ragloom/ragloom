# Ragloom

Minimalist RAG ingestion for local files, deterministic pipelines, and Qdrant-ready vector indexing.

![Rust](https://img.shields.io/badge/Rust-2024-000000?logo=rust)
![RAG](https://img.shields.io/badge/RAG-ingestion-1f6feb)

Ragloom is a minimalist RAG ingestion engine for local files. It scans a directory, detects changed file versions, loads UTF-8 text, chunks it, sends chunks to an embedding backend, and upserts deterministic points into Qdrant.

The project is split into a reusable Rust library and a thin CLI runner. The library exposes small traits for sources, document loading, embedding providers, and sinks so the pipeline can stay easy to test and extend.

## Features

- Minimal end-to-end ingestion pipeline for local files
- Deterministic point IDs for idempotent Qdrant upserts
- Polling-based file discovery with cheap file-version fingerprints
- Structured observability with pretty or JSON tracing output
- Replaceable embedding and sink backends through traits
- Integration and concurrency tests for runtime, chunking, observability, and sink behavior

## Architecture

The runtime is organized as a small in-process pipeline:

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

Core crates and modules:

- `source`: discovery of file versions from the filesystem
- `pipeline`: planning, runtime loop, worker execution, and acknowledgements
- `doc`: document loading abstraction and filesystem UTF-8 loader
- `transform`: chunking and chunk metadata generation
- `embed`: OpenAI and generic HTTP embedding clients
- `sink`: vector sink abstraction and Qdrant implementation
- `state`: in-memory WAL record types used by the MVP runtime
- `observability`: tracing subscriber configuration from environment variables

## Current Scope

Ragloom currently focuses on a small, explicit MVP:

- Local filesystem input only
- Polling directory scan only
- Top-level files in the configured directory only
- UTF-8 document loading only
- Qdrant as the only built-in sink
- OpenAI and generic HTTP embedding backends
- In-memory WAL only

That makes the current binary a good fit for prototyping, local automation, and validating ingestion behavior before adding more operational features.

## Quick Start

### Requirements

- Rust toolchain with edition 2024 support
- A running Qdrant instance
- An existing Qdrant collection with a vector size that matches your embedding model
- A directory containing UTF-8 text files

Build the binary:

```bash
cargo build --release
```

Run with the default OpenAI backend:

```bash
ragloom \
	--dir ./docs \
	--qdrant-url http://localhost:6333 \
	--collection docs \
	--openai-api-key "$OPENAI_API_KEY"
```

Run with a generic HTTP embedding service:

```bash
ragloom \
	--dir ./docs \
	--embed-backend http \
	--embed-url http://localhost:8080/embed \
	--embed-model default \
	--qdrant-url http://localhost:6333 \
	--collection docs
```

On Windows PowerShell, the same command looks like this:

```powershell
.\target\release\ragloom.exe `
	--dir .\docs `
	--qdrant-url http://localhost:6333 `
	--collection docs `
	--openai-api-key $env:OPENAI_API_KEY
```

The process runs until interrupted with Ctrl+C.

## CLI Usage

### Required flags

- `--dir <path>`: directory to scan
- `--qdrant-url <url>`: Qdrant base URL
- `--collection <name>`: Qdrant collection name

### Embedding backend selection

`--embed-backend` defaults to `openai`.

OpenAI backend options:

- `--openai-endpoint <url>`: defaults to `https://api.openai.com/v1/embeddings`
- `--openai-api-key <key>`: required when using the OpenAI backend
- `--openai-model <model>`: defaults to `text-embedding-3-small`

Generic HTTP backend options:

- `--embed-url <url>`: required when `--embed-backend http`
- `--embed-model <model>`: defaults to `default`

Flags support both `--flag value` and `--flag=value` forms.

## Embedding Backends

### OpenAI

The built-in OpenAI client sends requests to the embeddings API using this logical shape:

```json
{
	"model": "text-embedding-3-small",
	"input": ["chunk one", "chunk two"]
}
```

### Generic HTTP backend

The generic HTTP client expects a JSON API with this request and response shape:

Request:

```json
{
	"model": "default",
	"input": ["chunk one", "chunk two"]
}
```

Response:

```json
{
	"embeddings": [[1.0, 2.0], [3.0, 4.0]]
}
```

## Qdrant Integration

Ragloom writes points with deterministic IDs derived from the canonical path and chunk index. This keeps upserts idempotent and makes retries safe at the sink layer.

Each point payload includes document and chunk metadata such as:

- canonical file URI
- stable document ID hash
- file extension
- file size and mtime
- chunk index and total chunk count
- previous and next chunk IDs
- chunk byte offsets
- chunk text length
- chunk text hash
- chunk text content

Ragloom does not create collections automatically. You need to provision the target collection yourself with a vector size compatible with the selected embedding backend.

## Observability

Logging is configured through environment variables:

- `RAGLOOM_LOG_FORMAT`: `pretty` or `json`, default is `pretty`
- `RAGLOOM_LOG`: tracing filter directives, default is `info`

Examples:

```bash
RAGLOOM_LOG=debug cargo run -- --dir ./docs --qdrant-url http://localhost:6333 --collection docs --openai-api-key "$OPENAI_API_KEY"
```

```bash
RAGLOOM_LOG_FORMAT=json RAGLOOM_LOG=info cargo run -- --dir ./docs --qdrant-url http://localhost:6333 --collection docs --openai-api-key "$OPENAI_API_KEY"
```

The runtime emits tracing events around discovery, document loading, embedding requests, sink writes, and successful ingest completion.

## Library Usage

The crate is designed so you can reuse the pipeline pieces directly in Rust.

Add the dependency:

```toml
[dependencies]
ragloom = { git = "https://github.com/ragloom/ragloom" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Typical extension points:

- implement `source::Source` for a new discovery backend
- implement `doc::DocumentLoader` for remote or non-filesystem content
- implement `embed::EmbeddingProvider` for another model provider
- implement `sink::Sink` for another vector store or downstream system

The current binary in `src/main.rs` is a reference composition of these traits.

## Development

Run the test suite:

```bash
cargo test
```

Run benchmark targets:

```bash
cargo bench
```

Run loom-specific tests:

```bash
cargo test --features loom
```

Generate coverage if you use `cargo-llvm-cov`:

```bash
cargo llvm-cov --all --lcov --output-path lcov.info
```

## Limitations

- The directory scanner only scans one level and ignores nested directories.
- File change detection uses path, size, and mtime rather than content hashing.
- The built-in loader only reads UTF-8 files.
- The WAL is in memory, so it does not survive process restarts.
- The binary is currently CLI-driven; the typed YAML config module is present in the library but is not wired into the executable path.
- There is no built-in collection management, health endpoint, retry queue, or dead-letter handling.

## Roadmap Ideas

- Persistent WAL storage
- Recursive scanning or alternate source backends
- More sinks and embedding providers
- Runtime config reload integration
- Operational endpoints and richer retry controls

## Project Governance

- See `CONTRIBUTING.md` for local verification and development expectations.
- See `SUPPORT.md` for platform and release support policy.
- See `SECURITY.md` for vulnerability reporting guidance.

## Contributing

Issues and pull requests are welcome. Keep changes small, test-backed, and aligned with the project's minimalist design goals.

## License

Apache-2.0
