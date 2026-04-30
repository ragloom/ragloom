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
(for example `0.1.0`).

The release workflow verifies that:

- the requested version matches `Cargo.toml` `package.version`
- any pushed tag matches the same crate version
- the crate publish workflow re-checks that version/tag pair before `cargo publish`

Manual `push` of a `v*` tag is still supported, but it goes through the same
`Cargo.toml` consistency guard before release artifacts or crates.io publish
steps run.

GitHub Release notes are generated automatically by the release workflow so the
published notes come from the repository event history rather than ad hoc local
release text.

## Support Scope

The project treats Linux and Windows release targets as the formal support boundary for CI, release verification, and issue triage priority.

macOS binaries are provided as convenience artifacts. They should compile and publish when practical, but breakage on macOS does not block release unless maintainers explicitly promote it to a supported target.

## Getting Help

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
