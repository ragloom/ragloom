# Ragloom

A tiny Logstash-like ingestion daemon for RAG.

Point Ragloom at a folder. It watches local files, chunks documents, generates embeddings, and upserts deterministic points into Qdrant.

Use it when you want a small, inspectable ingestion pipeline instead of a full RAG platform.

![Rust](https://img.shields.io/badge/Rust-2024-000000?logo=rust)
![Status](https://img.shields.io/badge/status-alpha-b36b00)

## Why Ragloom?

Most RAG tools are full frameworks or platforms. Ragloom only handles ingestion.

It is built for developers who want to:

- keep a vector database in sync with local documents
- rerun ingestion safely without duplicate chunks
- version chunking strategies explicitly
- inspect what was indexed and why
- avoid adopting a full RAG framework

## Status

Ragloom is currently alpha software.

It is useful for local-folder to Qdrant ingestion experiments and small automation tasks.

Supported today:

- local filesystem source
- top-level files in one configured directory
- UTF-8 text, Markdown, and source code files
- recursive, Markdown-aware, and code-aware chunking
- experimental semantic chunking
- OpenAI and generic HTTP embedding APIs
- Qdrant sink
- deterministic point IDs
- pretty and JSON structured logs

Not supported yet:

- PDF or DOCX parsing
- recursive directory scanning
- persistent WAL
- automatic Qdrant collection creation
- production retry or dead-letter queues
- built-in collection lifecycle management

## Quickstart

This example runs Ragloom from source against a local Qdrant instance and the default OpenAI embedding backend.

### 1. Start Qdrant

```bash
docker run -d --name ragloom-qdrant -p 6333:6333 qdrant/qdrant
```

### 2. Create a collection

Ragloom does not create collections automatically. The example below assumes the default OpenAI model, `text-embedding-3-small`, which uses 1536-dimensional vectors.

```bash
curl -X PUT http://localhost:6333/collections/docs \
  -H "Content-Type: application/json" \
  -d '{"vectors":{"size":1536,"distance":"Cosine"}}'
```

If you use a different embedding model, create the collection with that model's vector size instead.

### 3. Prepare example documents

```bash
mkdir -p docs
printf "Ragloom watches files and indexes chunks into Qdrant.\n" > docs/intro.md
```

### 4. Run Ragloom

```bash
cargo run --release -- \
  --dir ./docs \
  --qdrant-url http://localhost:6333 \
  --collection docs \
  --openai-api-key "$OPENAI_API_KEY"
```

### 5. Expected result

Success looks like this:

- Ragloom starts and keeps running until you stop it with `Ctrl+C`
- you see startup and ingestion logs instead of a `ragloom.fatal` error
- points appear in the Qdrant collection `docs`

## Installation

Prebuilt binaries are not published yet. For now, install from source with Cargo.

### Build from source

```bash
git clone https://github.com/ragloom/ragloom
cd ragloom
cargo build --release
```

The compiled binary will be available at:

```text
target/release/ragloom
```

### Install into Cargo's bin directory

```bash
git clone https://github.com/ragloom/ragloom
cd ragloom
cargo install --path .
```

## Configuration

Ragloom supports a small typed YAML config for source, embed, and sink wiring.

### Basic configuration

```yaml
source:
  root: "./docs"

embed:
  endpoint: "https://api.openai.com/v1/embeddings"

sink:
  qdrant_url: "http://localhost:6333"
  collection: "docs"
```

Run with:

```bash
ragloom --config ./ragloom.yaml --openai-api-key "$OPENAI_API_KEY"
```

### Generic HTTP embedding

For a generic HTTP embedding service:

```yaml
embed:
  endpoint: "http://localhost:8080/embed"
```

```bash
ragloom --config ./ragloom.yaml --embed-backend http --embed-model default
```

### Configuration notes

- `--config` can provide `source.root`, `embed.endpoint`, `sink.qdrant_url`, and `sink.collection`
- backend-specific auth still comes from CLI flags, such as `--openai-api-key`
- chunker settings are currently configured by CLI flags, not by YAML
- flags support both `--flag value` and `--flag=value`
- the config file is merged with CLI flags; CLI flags take precedence

## How is Ragloom different?

Ragloom is not a RAG framework, chatbot, document QA app, or observability platform.

It only focuses on ingestion.

| Tool type | Examples | Focus |
| --- | --- | --- |
| RAG frameworks | LangChain, LlamaIndex | app orchestration |
| RAG platforms | RAGFlow, AnythingLLM | end-user RAG apps |
| document parsers | Unstructured, Docling | parsing documents |
| vector databases | Qdrant, Milvus, Weaviate | storing vectors |
| Ragloom | - | syncing documents into a vector DB |

Ragloom is for people who already have an app and a vector database, but want a small ingestion process in between.

## Core Concepts

### Source

Discovers document versions from a location such as a local folder.

### Loader

Reads document content. The built-in loader reads UTF-8 files from disk.

### Chunker

Splits documents into indexable chunks and records chunk metadata.

### Embedder

Turns chunks into vectors through OpenAI or a generic HTTP embedding API.

### Sink

Writes vectors and metadata into a destination such as Qdrant.

### State

Tracks discovered work and acknowledgements in an in-memory WAL.

## Architecture

```text
local folder
  ->
scanner
  ->
planner
  ->
WAL work items
  ->
runtime queue
  ->
loader
  ->
chunker
  ->
embedder
  ->
qdrant sink
  ->
acknowledgement
```

The implementation is intentionally split into small modules such as `source`, `doc`, `transform`, `embed`, `sink`, `pipeline`, and `observability`, but the runtime behavior stays narrow: discover files, turn them into chunks, embed them, and upsert them.

## Safe Reruns With Deterministic IDs

Ragloom generates deterministic point IDs from:

- canonical file path
- chunk index
- chunker strategy fingerprint

This makes reruns safe.

The same file and same chunking config produce the same point IDs. Changing chunking parameters creates a new ID space, so old chunks are not silently overwritten by new content.

## Chunking

Ragloom supports several chunking modes:

| Mode | Use case |
| --- | --- |
| `recursive` | general text |
| `markdown` | heading-aware Markdown splitting |
| `code:<lang>` | tree-sitter based source-code splitting |
| `semantic` | experimental sentence-level semantic splitting |

By default, Ragloom runs in router mode and chooses a chunker by file extension:

- `.md`, `.markdown`, `.mdx` -> Markdown chunker
- `.rs`, `.py`, `.js`, `.ts`, `.tsx`, `.go`, `.java`, `.c`, `.cpp`, `.rb`, `.sh` -> code chunker
- other files -> recursive chunker

Useful flags:

- `--chunker-mode router` keeps extension-based routing
- `--chunker-mode single --chunker-single recursive|markdown|semantic|code:<lang>` forces one chunker
- `--size-metric chars|tokens` chooses chunk sizing mode
- `--size-max`, `--size-min`, and `--size-overlap` tune boundaries
- `--enable-semantic` enables semantic chunking in router mode
- `--semantic-provider adapter|fastembed` selects the semantic signal source

Semantic chunking is opt-in. `fastembed` requires building with `--features fastembed`.

## Indexed Payload

Each Qdrant point includes chunk text plus metadata such as:

```json
{
  "canonical_path": "file:///Users/me/docs/intro.md",
  "doc_id": "doc_...",
  "tenant_id": "default",
  "file_extension": "md",
  "size_bytes": 842,
  "mtime_unix_secs": 1714300000,
  "chunk_index": 0,
  "total_chunks": 3,
  "previous_chunk_id": null,
  "next_chunk_id": "chunk_...",
  "chunk_start_byte": 0,
  "chunk_end_byte": 842,
  "chunk_char_len": 842,
  "chunk_text_sha256": "sha256_...",
  "strategy_fingerprint": "markdown:v1|...",
  "chunk_text": "..."
}
```

This is the part of Ragloom that makes inspection easier: you can look at a point in Qdrant and see where it came from, how it was chunked, and which neighboring chunks surround it.

## Observability

Ragloom emits `tracing` events for discovery, startup, embedding, Qdrant writes, and completion.

Environment variables:

- `RAGLOOM_LOG_FORMAT=pretty|json`
- `RAGLOOM_LOG=info|debug|...`

Example:

```bash
RAGLOOM_LOG_FORMAT=json RAGLOOM_LOG=info ragloom --config ./ragloom.yaml --openai-api-key "$OPENAI_API_KEY"
```

Ragloom does not log secrets, API keys, or full document contents.

## Roadmap

### v0.1 - First-run experience

- example environment for local Qdrant setup
- clearer ingestion summary at runtime
- automatic Qdrant collection creation
- recursive directory scanning
- release binaries

### v0.2 - More reliable daemon behavior

- persistent local state
- retry queue
- delete detection
- health endpoint
- metrics endpoint

### v0.3 - More document coverage

- PDF
- HTML
- DOCX
- frontmatter metadata
- external parser integrations

## Limitations

Ragloom is intentionally small today.

## Troubleshooting

### Ragloom fails to start with Qdrant connection error

Make sure Qdrant is running and accessible:

```bash
curl http://localhost:6333/health
```

If using Docker, verify the container is running:

```bash
docker ps | grep qdrant
```

### Collection not found error

Ragloom does not create collections automatically. Create the collection before running:

```bash
curl -X PUT http://localhost:6333/collections/docs \
  -H "Content-Type: application/json" \
  -d '{"vectors":{"size":1536,"distance":"Cosine"}}'
```

Adjust the vector size to match your embedding model.

### Empty or missing chunks

Check that your files are:
- UTF-8 encoded
- located in the top-level of the configured directory
- not in subdirectories (recursive scanning is not yet supported)

### OpenAI API errors

Verify your API key is set correctly:

```bash
echo $OPENAI_API_KEY
```

Test the embedding endpoint directly:

```bash
curl https://api.openai.com/v1/embeddings \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"input":"test","model":"text-embedding-3-small"}'
```

## Current limitations

- only local filesystem input
- only top-level files in the configured directory
- only Qdrant as a built-in sink
- only UTF-8 file loading
- no persistent WAL yet
- no automatic collection management yet
- no production retry queue yet

## Contributing

Ragloom is maintainer-led and intentionally small.

Good contributions include:

- bug fixes
- tests
- documentation
- examples
- small focused connectors
- improvements to first-run experience

Please open an issue before starting large features.

Before opening a pull request, run:

```bash
cargo qa
```

See `CONTRIBUTING.md` for development expectations, `SUPPORT.md` for support policy, and `SECURITY.md` for vulnerability reporting.

## License

Apache-2.0
