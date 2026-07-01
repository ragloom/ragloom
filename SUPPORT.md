# Support Policy

## Supported Platforms

Officially supported release targets:

- Linux x86_64
- Linux aarch64
- Windows x86_64

Best-effort release artifacts:

- macOS x86_64
- macOS aarch64

## Release Channels

Ragloom publishes artifacts through:

- GitHub Release binaries
- crates.io crate releases
- GHCR container images for Linux targets only

## Release Runbook

Maintainers should start releases from `.github/workflows/release.yml` using
`workflow_dispatch` with an explicit crate version from `Cargo.toml`
(for example `0.4.1`).

The release workflow verifies that:

- the requested version matches `Cargo.toml` `package.version`
- any pushed tag matches the same crate version
- the crate publish workflow re-checks that version/tag pair before `cargo publish`
- Linux and Windows release targets complete before GitHub Release and crates.io publication continue

Manual `push` of a `v*` tag is still supported, but it goes through the same
`Cargo.toml` consistency guard before release artifacts or crates.io publish
steps run.

GitHub Release notes are generated automatically by the release workflow so the
published notes come from the repository event history rather than ad hoc local
release text.

GitHub Release archives are published with explicit target names such as
`ragloom-v0.4.1-x86_64-unknown-linux-gnu.tar.gz` and
`ragloom-v0.4.1-x86_64-pc-windows-msvc.zip`.

Each published archive also includes a matching `.sha256.txt` checksum file.
Release checksums are generated with a platform-aware command so Linux targets
use `sha256sum` while macOS targets use `shasum -a 256`.

## v0.4 Support Readiness

For the current `v0.4` support surface, maintainers should treat the following
as release-blocking for the commit being released:

- `ci` is green for the release commit or release branch tip
- `quality-deep` is green for the same commit, or maintainers have run the equivalent local checks from `cargo maintainer-qa`
- the release workflow version and tag consistency guards pass
- supported Linux and Windows artifacts build successfully before GitHub Release and crates.io publication continue
- any release-tracked security or dependency finding has been resolved or explicitly deferred with a documented rationale in the release notes or tracking issue

The following are not release-blocking by default:

- macOS artifact failures, unless maintainers explicitly promote macOS to a supported release target
- experimental semantic chunking behavior, as long as the default non-semantic ingest path remains healthy

## Support Scope

The project treats Linux and Windows release targets as the formal support boundary for CI, release verification, and issue triage priority.

macOS binaries are provided as convenience artifacts. They should compile and publish when practical, but breakage on macOS does not block release unless maintainers explicitly promote it to a supported target.

When a macOS build succeeds, its archive is appended to the existing GitHub
Release after the supported Linux and Windows assets have already published.

State-journal compaction is part of that Linux and Windows support boundary.
On Unix targets, compaction uses same-directory rename plus parent-directory
sync after writing and syncing the temporary file. On Windows, compaction uses
the native replacement primitive for existing files. If replacement fails, the
original journal is the durability boundary and the command returns a `state`
error instead of dropping records.

The on-disk state compatibility contract is also part of that support boundary.
`v0.5.0` directly reads supported released `v0.4.x` WAL state, with `v0.4.0`
as the minimum supported WAL version. `failed.ndjson` is part of that contract
starting in `v0.4.1`, when the failed-work journal first became a released
state surface.

Unknown future record variants, malformed lines, truncated final writes, and
other unsupported state shapes fail closed with a `state` error. Maintainers
should treat any future incompatible state change as requiring an explicit,
documented migration boundary rather than a silent format reinterpretation.

## v0.5 compatibility boundary

The v0.5 compatibility boundary covers operator-visible point identity, Qdrant
payload shape, CLI defaults, and durable state upgrades. Undocumented internal
Rust APIs are outside this support promise.

The Qdrant point ID is the chunk identity. It is derived from canonical source
identity, chunk index, and the exact chunker strategy fingerprint.
Strategy-fingerprint changes intentionally open a new point-ID space.
Ragloom must never silently reuse an old point ID for different chunk content.
Operators who need a clean collection should reindex and then drop or
garbage-collect the old point-ID space.
The identity is not duplicated as a `chunk_id` payload field.

The stable v0.5 payload fields are `canonical_path`, `doc_id`, `tenant_id`,
`file_extension`, `size_bytes`, `mtime_unix_secs`, `chunk_index`,
`total_chunks`, `previous_chunk_id`, `next_chunk_id`, `chunk_start_byte`,
`chunk_end_byte`, `chunk_char_len`, `chunk_text_sha256`, and
`strategy_fingerprint`. Their names, presence, and JSON value kinds are
supported. `chunk_text` is optional compatibility data: v0.5 emits it, but
payload consumers should tolerate its omission. Additive fields are
non-contractual until this policy names them as stable.
Identity, extension, hash, and strategy fields are strings; size, time, index,
count, offset, and length fields are non-negative integers; neighboring chunk
IDs are either UUID strings or `null`.

The default embedding backend remains OpenAI with
`https://api.openai.com/v1/embeddings` and `text-embedding-3-small`; the
default chunker mode remains `router` with character sizing at `max=2000`,
`min=0`, and `overlap=0`. The default state path remains `.ragloom/wal.ndjson`,
collection bootstrap and health remain disabled by default, and retry defaults
remain 3 attempts, 128 queued retries, 100 ms initial backoff, and 2000 ms
maximum backoff. Semantic chunking remains experimental and opt-in.

Additive payload fields and fixes that preserve the documented identity,
payload, default, and state contracts are compatible. Removing or renaming a
stable field, changing a default, reusing an ID for different content, or
making released state unreadable is incompatible. Incompatible changes require release-note migration guidance.

| Upgrade observation | Operator action |
| --- | --- |
| Point IDs, stable payload fields, defaults, and released state remain compatible | No action |
| Strategy fingerprint changes | Reindex; drop or garbage-collect the old point-ID space if a clean collection is required |
| Stable payload field or CLI default changes | Follow the release notes and update consumers or configuration |
| Released state is not directly readable | Back up the state directory and complete the documented migration before startup |

## Feature Boundaries

Core support boundary maintainers are hardening for the current `v0.4`
support boundary:

- local filesystem ingestion under one configured directory, plus polling S3 ingestion under one configured bucket/prefix
- UTF-8 text, Markdown, source code, deterministic PDF text loading, and deterministic DOCX text extraction
- recursive, Markdown-aware, and code-aware chunking
- OpenAI and generic HTTP embedding backends
- Qdrant sink behavior, deterministic point IDs, local WAL replay, bounded retry, and the loopback-only health and metrics endpoint

Feature-gated paths:

- `fastembed` support is opt-in at build time and must keep passing its dedicated checks, but it is not the default shipped runtime path
- `fastembed` remains a feature-gated semantic provider

Experimental or best-effort paths:

- semantic chunking remains experimental and opt-in even when the required provider path is available
- semantic chunking remains experimental and opt-in
- macOS release artifacts remain convenience builds rather than part of the formal support contract

Semantic chunking is only active with `--enable-semantic` in router mode or with
`--chunker-mode single --chunker-single semantic`. When active, `--semantic-provider`
and `--semantic-percentile` are part of the experimental semantic surface in both modes.
In router mode, semantic chunking replaces the Markdown and generic text fallback paths
while code extensions continue to use the code chunker.

Out of scope for the current support contract:

- broader parser guarantees beyond built-in DOCX text extraction
- non-local operator surfaces
- collection lifecycle management beyond optional first-run bootstrap of the configured target collection

Current PDF support is limited to embedded text extraction. OCR, rich layout
reconstruction, and guarantees for image-only, scan-only, encrypted, or
otherwise unsupported PDFs remain outside the current support contract.

Current DOCX support is limited to deterministic extracted text. Ragloom
linearizes paragraphs and tables into chunkable text, but does not preserve
rich formatting, embedded assets, or full page layout fidelity.

## Getting Help

### State preflight and replay diagnostics

Run `ragloom check` with the normal runtime flags to validate local durable state
before ingest startup. It reports the configured state path and whether the WAL
and failed-work journal are readable and writable, or missing but safely
creatable. The command does not create or append state.

`ragloom replay-failed --state-path <path>` reports deterministic `pending`,
`requeued`, `skipped`, and `failed` counts. A non-zero exit status means replay
could not complete safely; preserve the journals and include the sanitized error
context when requesting support.

### Check the documentation first

- Review `README.md` for quickstart and configuration
- Review `CONTRIBUTING.md` for development guidelines
- Review `AGENTS.md` for AI coding agent guidance

### Open an issue

When opening an issue, please include:

- Ragloom version (`ragloom --version` or `ragloom -V`; source checkouts may also check `Cargo.toml`)
- Rust version (`rustc --version`)
- Operating system and architecture
- whether the missing files are nested under subdirectories or behind symbolic links
- Qdrant version (if applicable)
- Embedding backend and model used
- Steps to reproduce the issue
- Relevant log output (without API keys or secrets)

### Community resources

- GitHub Issues: https://github.com/ragloom/ragloom/issues
- GitHub Discussions: https://github.com/ragloom/ragloom/discussions

### Security issues

For security vulnerabilities, please follow `SECURITY.md` and do **not** open a public issue.
