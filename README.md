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
- recursive scanning of regular files under one configured directory
- UTF-8 text, Markdown, and source code files
- recursive, Markdown-aware, and code-aware chunking
- experimental semantic chunking
- OpenAI and generic HTTP embedding APIs
- Qdrant sink
- deterministic point IDs
- persistent local WAL state
- pretty and JSON structured logs

Not supported yet:

- PDF or DOCX parsing
- production retry or dead-letter queues
- built-in collection lifecycle management

## Quickstart

This example runs Ragloom from source against a local Qdrant instance and the default OpenAI embedding backend.

### 1. Start Qdrant

```bash
docker run -d --name ragloom-qdrant -p 6333:6333 qdrant/qdrant
```

### 2. Prepare example documents

```bash
mkdir -p docs
printf "Ragloom watches files and indexes chunks into Qdrant.\n" > docs/intro.md
```

### 3. Run Ragloom

```bash
cargo run --release -- \
  --dir ./docs \
  --qdrant-url http://localhost:6333 \
  --collection docs \
  --state-path ./.ragloom/wal.ndjson \
  --create-collection-if-missing \
  --openai-api-key "$OPENAI_API_KEY"
```

With the default OpenAI model, `text-embedding-3-small`, Ragloom can infer the Qdrant vector size automatically during bootstrap.

Pass `--collection-vector-size <n>` when Ragloom cannot infer the size for your embedding backend or model:

- required for `--embed-backend http`
- required for unknown or custom OpenAI embedding models
- optional if you want to override the inferred size explicitly

Example with an explicit size:

```bash
cargo run --release -- \
  --dir ./docs \
  --qdrant-url http://localhost:6333 \
  --collection docs \
  --create-collection-if-missing \
  --collection-vector-size 1536 \
  --openai-api-key "$OPENAI_API_KEY"
```

Ragloom only bootstraps the target collection when it is missing. It does not manage broader collection lifecycle tasks such as reconfiguration, deletion, migrations, or index tuning.

### 4. Expected result

Success looks like this:

- Ragloom starts and keeps running until you stop it with `Ctrl+C`
- you see startup and ingestion logs instead of a `ragloom.fatal` error
- you see a structured `ragloom.ingest.summary` event with counts such as `discovered_files`, `indexed_files`, `emitted_points`, and `failed_files`
- points appear in the Qdrant collection `docs`

## Installation

Ragloom publishes GitHub Release binaries for supported platforms:

- `ragloom-v<version>-x86_64-unknown-linux-gnu.tar.gz`
- `ragloom-v<version>-aarch64-unknown-linux-gnu.tar.gz`
- `ragloom-v<version>-x86_64-pc-windows-msvc.zip`

Best-effort macOS assets are published with the same naming pattern when those jobs succeed:

- `ragloom-v<version>-x86_64-apple-darwin.tar.gz`
- `ragloom-v<version>-aarch64-apple-darwin.tar.gz`

Each release also includes a matching `.sha256.txt` checksum file and `.spdx.json` SBOM.

### Install a release binary

Download the archive for your platform from the GitHub Release page, extract it, and run `ragloom` (or `ragloom.exe` on Windows).

Examples:

```bash
tar -xzf ragloom-v0.1.1-x86_64-unknown-linux-gnu.tar.gz
./ragloom --version
```

```powershell
Expand-Archive .\ragloom-v0.1.1-x86_64-pc-windows-msvc.zip -DestinationPath .
.\ragloom.exe --version
```

If you prefer or need an unsupported target, install from source with Cargo.

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

Verify the built binary version with:

```bash
target/release/ragloom --version
```

### Install into Cargo's bin directory

```bash
git clone https://github.com/ragloom/ragloom
cd ragloom
cargo install --path .
```

Then confirm the installed executable with `ragloom --version` or `ragloom -V`.

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

state:
  path: ".ragloom/wal.ndjson"
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
- `--config` can also provide `state.path`; the CLI flag is `--state-path`
- backend-specific auth still comes from CLI flags, such as `--openai-api-key`
- chunker settings are currently configured by CLI flags, not by YAML
- flags support both `--flag value` and `--flag=value`
- the config file is merged with CLI flags; CLI flags take precedence

### Source scanning behavior

The built-in filesystem source walks the configured root recursively and ingests
regular files it can stat.

- traversal is deterministic because directory entries are processed in sorted path order
- hidden files and hidden directories are treated like any other path
- symbolic links are not followed
- unreadable directories or files that cannot be stat'ed are skipped

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

Tracks discovered work and acknowledgements in an append-only local WAL.

By default, Ragloom stores state at `.ragloom/wal.ndjson` relative to the
current working directory. Pass `--state-path <path>` or set `state.path` in the
YAML config to choose another file.

The WAL is newline-delimited JSON and records planned `WorkItemV2` entries plus
`SinkAckV2` acknowledgements. On startup, Ragloom replays work items that do not
have a matching acknowledgement and seeds planner de-duplication from the WAL so
already acknowledged file versions are not planned again. Corrupt or unreadable
state files fail startup with a `state` error instead of being ignored.

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

Ragloom emits `tracing` events for discovery, startup, embedding, Qdrant writes, and ingest completion summaries.

Environment variables:

- `RAGLOOM_LOG_FORMAT=pretty|json`
- `RAGLOOM_LOG=info|debug|...`

Example:

```bash
RAGLOOM_LOG_FORMAT=json RAGLOOM_LOG=info ragloom --config ./ragloom.yaml --openai-api-key "$OPENAI_API_KEY"
```

Ragloom does not log secrets, API keys, or full document contents.

For first-run validation, look for `ragloom.ingest.summary`. Ragloom emits it after an ingest window goes idle and again on shutdown when there is still unreported work. The summary stays structured and includes counters such as:

- `discovered_files`
- `indexed_files`
- `failed_files`
- `emitted_points`
- `pending_files`

## Roadmap

### v0.1 - First-run experience

Status: shipped in `v0.1.1`.

- example environment for local Qdrant setup
- clearer ingestion summary at runtime
- release binaries

### v0.2 - More reliable daemon behavior

- persistent local state (shipped on `main`)
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

If you started Ragloom with `--create-collection-if-missing`, it will bootstrap the target collection on first run.

For the default OpenAI model, this is enough:

```bash
cargo run --release -- \
  --dir ./docs \
  --qdrant-url http://localhost:6333 \
  --collection docs \
  --create-collection-if-missing \
  --openai-api-key "$OPENAI_API_KEY"
```

If you are using `--embed-backend http` or an OpenAI model Ragloom does not recognize yet, rerun with an explicit vector size:

```bash
cargo run --release -- \
  --dir ./docs \
  --qdrant-url http://localhost:6333 \
  --collection docs \
  --create-collection-if-missing \
  --collection-vector-size 1536 \
  --embed-backend http \
  --embed-url http://localhost:8080/embed \
  --embed-model default
```

If you prefer to manage Qdrant yourself, pre-create the collection before running:

```bash
curl -X PUT http://localhost:6333/collections/docs \
  -H "Content-Type: application/json" \
  -d '{"vectors":{"size":1536,"distance":"Cosine"}}'
```

Adjust the vector size to match your embedding model. Ragloom does not perform general collection lifecycle management beyond optional first-run bootstrap of the configured collection.

### Empty or missing chunks

Check that your files are:
- UTF-8 encoded
- located somewhere under the configured directory
- regular files rather than symbolic links

If startup looks healthy but nothing appears in Qdrant, check the latest `ragloom.ingest.summary` event first. A non-zero `failed_files` or `pending_files` count usually narrows the problem down faster than scanning individual per-file log lines.

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
- only Qdrant as a built-in sink
- only UTF-8 file loading
- no general collection lifecycle management beyond optional first-run bootstrap
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
