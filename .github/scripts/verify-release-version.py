#!/usr/bin/env python3

from __future__ import annotations

import os
import sys
import tomllib
from pathlib import Path


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def append_output(name: str, value: str) -> None:
    github_output = os.environ.get("GITHUB_OUTPUT")
    if not github_output:
        return

    with open(github_output, "a", encoding="utf-8") as handle:
        handle.write(f"{name}={value}\n")


def main() -> None:
    repo_root = Path(__file__).resolve().parents[2]
    cargo_toml = repo_root / "Cargo.toml"
    manifest = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))

    try:
        crate_version = str(manifest["package"]["version"]).strip()
    except KeyError as error:
        fail(f"failed to read package.version from {cargo_toml}: {error}")

    expected_version = os.environ.get("EXPECTED_VERSION", "").strip()
    expected_tag = os.environ.get("EXPECTED_TAG", "").strip()

    if expected_tag:
        if not expected_tag.startswith("v"):
            fail(
                f"release tag must start with 'v'; got '{expected_tag}' while "
                f"verifying Cargo.toml version {crate_version}"
            )

        tag_version = expected_tag.removeprefix("v")
        if expected_version and expected_version != tag_version:
            fail(
                f"release version '{expected_version}' does not match tag "
                f"'{expected_tag}' derived version '{tag_version}'"
            )
        expected_version = tag_version

    if not expected_version:
        fail("EXPECTED_VERSION or EXPECTED_TAG must be provided")

    if crate_version != expected_version:
        fail(
            f"Cargo.toml package.version '{crate_version}' does not match "
            f"expected release version '{expected_version}'"
        )

    tag = expected_tag or f"v{expected_version}"
    append_output("crate_version", crate_version)
    append_output("version", expected_version)
    append_output("tag", tag)

    print(
        f"verified Cargo.toml package.version '{crate_version}' for release tag '{tag}'",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
