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

The v0.4 direction is to widen the ingestion surface carefully without losing
the project's explicit support boundaries.

Core path the project is hardening for the current v0.4 support boundary:

- local filesystem source
- polling S3 source
- recursive scanning of regular files under one configured directory
- UTF-8 text, Markdown, source code, text-extractable PDF loading, and deterministic DOCX text extraction
- recursive, Markdown-aware, and code-aware chunking
- OpenAI and generic HTTP embedding APIs
- Qdrant sink
- deterministic point IDs
- persistent local WAL state
- bounded in-process retry for transient ingest failures
- pretty and JSON structured logs
- opt-in local health and metrics endpoint

Feature-gated paths:

- `fastembed` local semantic signal support when built with `--features fastembed`

Experimental or best-effort paths:

- semantic chunking remains experimental and opt-in
- `fastembed` remains a feature-gated semantic provider
- macOS release artifacts remain best-effort convenience binaries rather than release-blocking targets

Not supported yet:

- broad job-management or dead-letter queue subsystems beyond the local failed-work journal
- built-in collection lifecycle management

### v0.4 support matrix

| Area | Supported in v0.4 | Explicitly out of scope |
| --- | --- | --- |
| sources | local filesystem, polling S3 | non-S3 remote sources |
| document loading | UTF-8 text, Markdown, source code, embedded-text PDF extraction, deterministic DOCX text extraction | OCR, rich layout reconstruction, broader office-suite parsing |
| operations | local health endpoint, local metrics endpoint, optional first-run collection bootstrap | non-local operator surfaces, broader collection lifecycle management |

### v0.5 compatibility boundary

The v0.5 compatibility boundary covers operator-visible point identity, Qdrant
payload shape, CLI defaults, and durable state upgrades. It does not guarantee
undocumented internal Rust APIs.

The Qdrant point ID is the chunk identity. It is derived from the canonical
source identity, chunk index, and exact chunker strategy fingerprint. The same
inputs keep the same ID. Strategy-fingerprint changes intentionally open a new point-ID space.
This prevents changed chunk boundaries or semantics from overwriting older
chunks under reused IDs. Operators should reindex and then drop or
garbage-collect old points when they want a clean collection. The identity is
not duplicated as a `chunk_id` payload field.

For v0.5, the stable Qdrant payload fields are `canonical_path`, `doc_id`,
`tenant_id`, `file_extension`, `size_bytes`, `mtime_unix_secs`, `chunk_index`,
`total_chunks`, `previous_chunk_id`, `next_chunk_id`, `chunk_start_byte`,
`chunk_end_byte`, `chunk_char_len`, `chunk_text_sha256`, and
`strategy_fingerprint`. Their names, presence, and JSON value kinds are the
compatibility contract. `chunk_text` is optional compatibility data: v0.5
emits it, but consumers should tolerate its omission. Newly added payload
fields are non-contractual until they are explicitly added to this list.
Identity, extension, hash, and strategy fields are strings; size, time, index,
count, offset, and length fields are non-negative integers; neighboring chunk
IDs are either UUID strings or `null`.

The default embedding backend remains OpenAI with
`https://api.openai.com/v1/embeddings` and `text-embedding-3-small`; the
default chunker mode remains `router`, using character sizing with
`max=2000`, `min=0`, and `overlap=0`. The default state path remains
`.ragloom/wal.ndjson`, collection bootstrap and the health endpoint remain
disabled, and the retry defaults remain 3 attempts, 128 queued retries, 100 ms
initial backoff, and 2000 ms maximum backoff. Semantic chunking remains
experimental and opt-in.

Additive payload fields and bug fixes that preserve these identities, fields,
defaults, and readable state are compatible. Removing or renaming a stable
payload field, changing a default, reusing an old ID for different chunk
content, or making released state unreadable is incompatible. Incompatible changes require release-note migration guidance.
See the state-specific rules below for the supported upgrade path.

| Upgrade observation | Operator action |
| --- | --- |
| Point IDs, stable payload fields, defaults, and released state remain compatible | No action |
| Strategy fingerprint changes | Reindex; drop or garbage-collect the old point-ID space if a clean collection is required |
| Stable payload field or CLI default changes | Follow the release notes; update payload consumers or pin the old configuration before upgrading |
| Released state is not directly readable | Back up the state directory and follow the documented migration before starting the new release |

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

To validate startup wiring without ingesting, use one of the non-ingesting command paths first:

```bash
cargo run --release -- check \
  --dir ./docs \
  --qdrant-url http://localhost:6333 \
  --collection docs \
  --create-collection-if-missing \
  --openai-api-key "$OPENAI_API_KEY"
```

```bash
cargo run --release -- dry-run \
  --dir ./docs \
  --qdrant-url http://localhost:6333 \
  --collection docs \
  --create-collection-if-missing \
  --openai-api-key "$OPENAI_API_KEY"
```

`check` validates configuration, source wiring, chunker selection, bootstrap prerequisites,
and local durable state without starting ingest. State preflight validates that existing WAL
and failed-work journals are readable, use supported record formats, and are writable for a
real run. Missing state directories succeed when their nearest existing parent is writable;
the check does not create them. `check` prints the state path and concise journal statuses.
`dry-run` performs the same validation and also prints the effective startup choices,
including source kind, chunker selection, and whether Ragloom would bootstrap the configured
collection. Neither command creates state, sends embeddings, or writes to Qdrant.

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

Each release also includes a matching `.sha256.txt` checksum file for archive verification.

### Install a release binary

Download the archive for your platform from the GitHub Release page, extract it, and run `ragloom` (or `ragloom.exe` on Windows).

Examples:

```bash
tar -xzf ragloom-v0.4.1-x86_64-unknown-linux-gnu.tar.gz
./ragloom --version
```

```powershell
Expand-Archive .\ragloom-v0.4.1-x86_64-pc-windows-msvc.zip -DestinationPath .
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
  kind: "filesystem"
  root: "./docs"

embed:
  endpoint: "https://api.openai.com/v1/embeddings"

sink:
  qdrant_url: "http://localhost:6333"
  collection: "docs"

state:
  path: ".ragloom/wal.ndjson"

retry:
  max_attempts: 3
  max_queued: 128
  initial_backoff_ms: 100
  max_backoff_ms: 2000

# Optional. Omit to keep the health endpoint disabled.
health:
  addr: "127.0.0.1:8080"
```

Run with:

```bash
ragloom --config ./ragloom.yaml --openai-api-key "$OPENAI_API_KEY"
```

### S3 source configuration

For polling S3 ingestion, use the canonical S3 config shape:

```yaml
source:
  kind: "s3"
  bucket: "docs-bucket"
  prefix: "kb/"

embed:
  endpoint: "https://api.openai.com/v1/embeddings"

sink:
  qdrant_url: "http://localhost:6333"
  collection: "docs"
```

Example CLI startup for the same source:

```bash
ragloom \
  --source-kind s3 \
  --s3-bucket docs-bucket \
  --s3-prefix kb/ \
  --qdrant-url http://localhost:6333 \
  --collection docs \
  --openai-api-key "$OPENAI_API_KEY"
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

- `source.kind: filesystem` plus `source.root` is the canonical filesystem config shape
- legacy filesystem config that only sets `source.root` remains supported for compatibility
- `source.kind: s3` plus `source.bucket` and optional `source.prefix` configures polling S3 ingestion
- `--config` can provide `source.kind`, `source.root`, `source.bucket`, `source.prefix`, `embed.endpoint`, `sink.qdrant_url`, and `sink.collection`
- `--config` can also provide `state.path`; the CLI flag is `--state-path`
- `--config` can provide `retry.max_attempts`, `retry.max_queued`, `retry.initial_backoff_ms`, and `retry.max_backoff_ms`
- `--config` can provide `health.addr`; the CLI flag is `--health-addr`
- `--dir` remains the filesystem CLI shorthand; `--source-kind filesystem --dir ./docs` is equivalent to filesystem config
- `--s3-bucket` and `--s3-prefix` require `--source-kind s3`
- S3 runtime auth and region come from the process environment; set `AWS_REGION` or `AWS_DEFAULT_REGION` plus your normal AWS credential chain inputs
- backend-specific auth still comes from CLI flags, such as `--openai-api-key`
- chunker settings are currently configured by CLI flags, not by YAML
- `check` and `dry-run` are non-ingesting command paths; `dry-run` reports effective startup choices and bootstrap prerequisites only
- flags support both `--flag value` and `--flag=value`
- the config file is merged with CLI flags; CLI flags take precedence
- when `--config` is in use, Ragloom polls the config file for changes once per second
- hot reload currently applies only to `retry.*`
- CLI-provided retry flags stay pinned across reloads; file edits only affect retry fields that still come from YAML
- changes to `source.*`, `embed.endpoint`, `sink.*`, `state.path`, and `health.addr` are rejected during reload and require a process restart
- invalid or rejected reloads are logged and the last good runtime config remains active

### Retry behavior

Ragloom retries transient loader I/O, embedding, and sink failures inside the
worker before marking a file version failed for the ingest window. Configuration
and invalid-input errors are not retried.

Defaults:

- `max_attempts: 3`
- `max_queued: 128`
- `initial_backoff_ms: 100`
- `max_backoff_ms: 2000`

CLI overrides:

- `--retry-max-attempts <n>`
- `--retry-max-queued <n>`
- `--retry-initial-backoff-ms <ms>`
- `--retry-max-backoff-ms <ms>`

Set `--retry-max-attempts 1` to disable retries. Backoff is deterministic and
jitter-free so tests and local runs remain reproducible.

### Source scanning behavior

Ragloom ships with two polling source shapes:

- filesystem: walks the configured root recursively and ingests regular files it can stat
- s3: lists objects under the configured bucket and optional prefix, then ingests them through the same planner/runtime flow

For S3 runtime ingestion, Ragloom treats object keys as opaque. The
canonical S3 document identity will be `s3://{bucket}/{exact-key}` without
normalizing duplicate slashes, dot segments, or the configured prefix.

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

Reads document content. The built-in loaders currently extract UTF-8 text,
deterministic PDF text, and deterministic DOCX text from local files and S3
objects behind the same
document-loading boundary.

PDF extraction is text-only. Ragloom does not perform OCR, does not reconstruct
rich layout, and may return an empty string for image-only or scan-only PDFs.
Encrypted or malformed PDFs fail at the loader boundary with project-level
errors.

DOCX extraction is also text-only. Ragloom linearizes paragraphs with newline
separators and table cells with tab separators, does not preserve rich
formatting or embedded assets, and fails malformed DOCX inputs at the loader
boundary with project-level errors.

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
have a matching acknowledgement, seeds planner de-duplication from the WAL so
already acknowledged file versions are not planned again, and restores the set
of previously observed document paths used by delete detection. This means
delete synchronization survives restarts as long as you reuse the same
`--state-path` or `state.path`. Corrupt or unreadable state files fail startup
with a `state` error instead of being ignored.
Ragloom keeps the WAL append handle open for the lifetime of the process to
avoid repeated open/close overhead, while still calling `sync_data()` after each
record so the durability boundary remains one acknowledged append at a time.

### State compatibility contract

Ragloom `v0.5.0` directly reads the supported released `v0.4.x` WAL format,
with `v0.4.0` as the minimum supported on-disk WAL version. `failed.ndjson`
enters that compatibility surface in `v0.4.1`; older state directories may not
have a failed-work journal yet.

These files are durable user-owned state. Ragloom does not silently skip or
repair unknown future record variants, malformed lines, truncated final writes,
or other unsupported state shapes. Instead, it fails closed with a `state`
error that identifies the failing file and line so operators can inspect the
journal intentionally.

If a future Ragloom release needs an incompatible state change, the project
will document that boundary explicitly and require a deliberate migration path
rather than silently reinterpreting older state.

Retries are not persisted as separate WAL records. If the process stops while
retries are queued, unacknowledged work is replayed from the WAL on the next
startup.

When a work item exhausts retries or fails terminally without retry, Ragloom
also appends a sanitized failed-work record to `failed.ndjson` in the same
directory as the WAL. Failed-work records include the original scheduled
`WalRecord`, retry attempt count, and a small failure classification only; they
do not include secrets, embedding payloads, or full document contents.

Inspect failed work directly from the local NDJSON file, for example:

```bash
cat .ragloom/failed.ndjson
```

To requeue pending failed work back into the WAL for operator replay, run:

```bash
ragloom replay-failed --state-path .ragloom/wal.ndjson
```

Or, if you already keep the state path in YAML:

```bash
ragloom replay-failed --config ./ragloom.yaml
```

`replay-failed` requires either `--state-path` or `--config`. It does not require
runtime ingest flags such as `--dir`, `--qdrant-url`, or `--collection`.
On success it prints deterministic `pending`, `requeued`, `skipped`, and `failed`
counts. `skipped` counts exhausted records already marked as requeued. An unsafe
or incomplete replay returns a non-zero exit status and includes the partial
summary in its error context.
Replay is intentionally at-least-once: if a prior replay appended work into the
WAL but crashed before marking the failed record requeued, rerunning
`replay-failed` may append that work again rather than silently losing it.

To compact both journals without changing replay semantics, run:

```bash
ragloom compact-state --state-path .ragloom/wal.ndjson
```

Or reuse YAML state configuration:

```bash
ragloom compact-state --config ./ragloom.yaml
```

`compact-state` is an explicit operator command. Ragloom does not compact state
implicitly during normal ingest. The command rewrites `wal.ndjson` and
`failed.ndjson` to the minimum records needed to preserve startup replay,
planner de-duplication, delete synchronization, pending failed work, and
`replay-failed` behavior.

Compaction writes a same-directory temporary file, flushes it with
`sync_data()`, and only then replaces the original journal. On Linux and other
Unix targets, Ragloom uses same-directory rename plus parent-directory sync. On
Windows, Ragloom uses the native file-replacement primitive so compaction can
replace an existing journal without first deleting it. If validation, writing,
or replacement fails, the original readable journal remains the safety
boundary and Ragloom returns a `state` error instead of silently discarding
records.

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

For the current filesystem source, the canonical document identity starts from
the local canonical path and is then rendered into a stable `file://` URI for
sink payloads and `doc_id` derivation. For the reserved S3 source shape, the
canonical document identity is defined directly as `s3://{bucket}/{exact-key}`.

This makes reruns safe.

The same file and same chunking config produce the same point IDs. Changing chunking parameters creates a new ID space, so old chunks are not silently overwritten by new content.

File-version identity is source-specific metadata hashed together with that
canonical document identity. Today the filesystem source uses path, size, and
mtime. The S3 design uses canonical S3 identity, object size, last-modified
time, and normalized ETag so the planner and WAL can preserve deterministic
replay semantics without requiring a per-object `HEAD` request.

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
- `--semantic-percentile <1..=99>` tunes semantic split sensitivity when semantic chunking is active

Semantic chunking remains experimental and opt-in. It is only active with
`--enable-semantic` in router mode or with `--chunker-mode single --chunker-single semantic`.
When active, `--semantic-provider` and `--semantic-percentile` apply to both router and
single semantic modes. In router mode, semantic chunking replaces the Markdown and generic
text fallback paths while code extensions continue to use the code chunker. `fastembed`
remains a feature-gated semantic provider and requires building with `--features fastembed`.

## Indexed Payload

Each Qdrant point currently includes chunk text plus metadata such as:

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
  "next_chunk_id": "9c5eb1c7-f9a7-4fc2-a260-761842356849",
  "chunk_start_byte": 0,
  "chunk_end_byte": 842,
  "chunk_char_len": 842,
  "chunk_text_sha256": "sha256_...",
  "strategy_fingerprint": "markdown:v1|...",
  "chunk_text": "..."
}
```

The Qdrant point ID is the chunk identity and is not duplicated as a
`chunk_id` payload field. `previous_chunk_id` and `next_chunk_id` contain those
same deterministic point-ID values for neighboring chunks.

This is the part of Ragloom that makes inspection easier: you can look at a point in Qdrant and see where it came from, how it was chunked, and which neighboring chunks surround it.

## Delete Synchronization

Ragloom tracks files it has previously observed under the configured source root. When a completed scan no longer sees one of those paths, the runtime plans a durable delete work item and the Qdrant sink removes all points with that document's stable `doc_id`.

Delete synchronization is idempotent and replay-safe: rerunning the same delete work is expected to leave Qdrant in the same state. Ragloom does not delete points for files it has never observed, and this is document-level cleanup only; it does not create, drop, or otherwise manage whole collections.

The same delete-sync model is reserved for S3: deletes are inferred only after
a completed successful listing of the configured bucket and prefix. Missing
objects from a partial or failed scan must not be treated as deletes.

For S3 object lifecycle semantics, Ragloom treats object identity as path-like:

- overwriting the same key is a new version of the same document when version metadata changes
- renaming a key is modeled as deleting the old key and ingesting a new document at the new key
- copying to a new key is modeled as a distinct document even if the content and ETag match

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

### Health endpoint

The local health endpoint is disabled by default. Enable it with either:

```bash
ragloom --config ./ragloom.yaml --health-addr 127.0.0.1:8080 --openai-api-key "$OPENAI_API_KEY"
```

or:

```yaml
health:
  addr: "127.0.0.1:8080"
```

The address must be an IP socket address on a loopback interface, such as
`127.0.0.1:8080` or `[::1]:8080`. Ragloom rejects non-loopback addresses to
avoid accidentally exposing the operator endpoint outside the local machine.

Query it with:

```bash
curl http://127.0.0.1:8080/health
```

Ready responses return HTTP `200` and a small JSON body with daemon status and
build/version information:

```json
{
  "status": "ready",
  "ready": true,
  "version": "0.4.0",
  "build": {
    "package": "ragloom",
    "version": "0.4.0"
  }
}
```

Startup/bootstrap failures and fatal runtime-loop failures return HTTP `503`
with `ready: false` and a short `reason` such as `startup_failed` or
`runtime_failed`. The endpoint does not include document text, API keys, or
full local paths.

### Metrics endpoint

When the local health listener is enabled, Ragloom also exposes metrics on the
same loopback address:

```bash
curl http://127.0.0.1:8080/metrics
```

The response uses the Prometheus text exposition format (`text/plain;
version=0.0.4`) so local monitoring tools can scrape it directly. Metrics are
numeric ingest and reliability counters only; they do not include document text,
API keys, or full local paths.

Current metrics:

- `ragloom_discovered_files_total`
- `ragloom_indexed_files_total`
- `ragloom_failed_files_total`
- `ragloom_emitted_points_total`
- `ragloom_pending_files`
- `ragloom_retry_attempts_total`
- `ragloom_retry_exhausted_total`
- `ragloom_retry_queue_depth`
- `ragloom_work_queue_depth`
- `ragloom_state_wal_bytes`
- `ragloom_state_failed_work_bytes`
- `ragloom_state_wal_pending_work`
- `ragloom_state_failed_work_pending`

Durable-state metrics are numeric only:

- `ragloom_state_wal_bytes` reports the current WAL file size in bytes
- `ragloom_state_failed_work_bytes` reports the current `failed.ndjson` size in bytes
- `ragloom_state_wal_pending_work` reports the current count of durable WAL work items awaiting acknowledgement
- `ragloom_state_failed_work_pending` reports the current count of durable failed-work items still pending operator replay

These metrics do not expose document text, API keys, canonical paths, local
paths, or failure-detail payloads. Ragloom updates them at startup and at
durable state transition points; it does not re-parse the full journals on each
metrics scrape.

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

Status: shipped in `v0.2.0`, with restart-safe delete synchronization refined in
`v0.2.1`.

- persistent local state
- bounded retry queue
- delete detection
- health endpoint
- metrics endpoint

### v0.4 - Explicit platform expansion boundaries

- keep the local-filesystem and polling-S3 ingest paths release-ready on supported Linux and Windows targets
- require green `ci` and `quality-deep` workflow states, or equivalent local maintainer verification, before cutting the release
- keep feature-gated paths such as `fastembed` exercised by dedicated checks without making them the default runtime path
- document PDF and DOCX extraction limits explicitly instead of implying broader parser guarantees
- keep remote-source scope narrow by supporting S3 only rather than expanding into a general remote-ingestion platform

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

Verify your API key is set without printing the value:

```bash
if [ -n "$OPENAI_API_KEY" ]; then echo "OPENAI_API_KEY is set"; else echo "OPENAI_API_KEY is not set"; fi
```

Test the embedding endpoint directly:

```bash
curl https://api.openai.com/v1/embeddings \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"input":"test","model":"text-embedding-3-small"}'
```

Do not share command output if it includes credentials or other sensitive response details.

## Current limitations

- only local filesystem and polling S3 input
- only Qdrant as a built-in sink
- only UTF-8 text, Markdown, source code, text-extractable PDF loading, and deterministic DOCX text extraction
- no general collection lifecycle management beyond optional first-run bootstrap
- no broad persistent dead-letter queue or job-management subsystem

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

Maintainers preparing release-sensitive or v0.4 support-boundary work should also run
the authoritative deeper local gate:

```bash
cargo maintainer-qa
```

`cargo qa` stays the fast contributor gate. `cargo maintainer-qa` extends it
with the checks that currently matter for local maintainer confidence:

- `cargo test --workspace --features loom`
- `cargo build --features fastembed`
- `cargo test --workspace --all-targets --features fastembed`
- `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS=-D warnings`
- `cargo deny check`
- `cargo audit`

GitHub Actions runs the same deeper stability checks on pull requests.

See `CONTRIBUTING.md` for development expectations, `SUPPORT.md` for support policy, and `SECURITY.md` for vulnerability reporting.

## License

Apache-2.0
