#!/usr/bin/env python3
"""Verify that the zarr-lint project version is consistent everywhere.

There is a single authoritative version: the Cargo workspace package version in
the root ``Cargo.toml``. Every crate inherits it with ``version.workspace =
true`` and the CLI reads it from Cargo at compile time. This script guards that
invariant so it cannot silently drift.

Checks performed:

1. Every workspace crate resolves to the same version.
2. The crates expected to be published are present.
3. The ``zarr-lint`` CLI reports that version (unless ``--skip-cli``).
4. If ``--tag`` is given, it equals ``v<version>`` (release gate).

Usage::

    python tools/check_versions.py
    python tools/check_versions.py --tag v0.0.1
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import tomllib
from pathlib import Path

# Crates that are part of the public release surface and must share the version.
PUBLISHABLE = {"zarr-lint-core", "zarr-lint-cli", "zarr-lint-python"}

# Accepts tags like ``v0.0.1`` or ``0.0.1`` with an optional pre-release/build
# suffix, mirroring the pattern used by the sibling arrow-lint project.
TAG_PATTERN = re.compile(r"^v?(?P<version>\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$")


def cargo_metadata() -> dict:
    output = subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        text=True,
    )
    return json.loads(output)


def workspace_versions(metadata: dict) -> dict[str, str]:
    members = set(metadata["workspace_members"])
    versions: dict[str, str] = {}
    for package in metadata["packages"]:
        if package["id"] in members:
            versions[package["name"]] = package["version"]
    return versions


def cli_reported_version() -> str:
    output = subprocess.check_output(
        ["cargo", "run", "--quiet", "--package", "zarr-lint-cli", "--", "--version"],
        text=True,
    ).strip()
    parts = output.split()
    if len(parts) != 2 or parts[0] != "zarr-lint":
        raise SystemExit(f"unexpected `--version` output: {output!r}")
    return parts[1]


def check_pyproject_dynamic_version() -> None:
    """Ensure the Python package derives its version dynamically.

    The wheel version must come from the Cargo crate via maturin, so
    ``pyproject.toml`` must declare ``version`` as dynamic and must not pin a
    static version that could drift.
    """
    pyproject_path = Path("pyproject.toml")
    if not pyproject_path.exists():
        return
    project = tomllib.loads(pyproject_path.read_text()).get("project", {})
    if "version" in project:
        raise SystemExit(
            "pyproject.toml [project] must not pin a static version; "
            "the version is derived from Cargo dynamically"
        )
    if "version" not in project.get("dynamic", []):
        raise SystemExit('pyproject.toml [project] must declare dynamic = ["version"]')


def version_from_tag(tag: str) -> str:
    match = TAG_PATTERN.match(tag)
    if not match:
        raise SystemExit(f"release tag must look like v1.2.3: {tag}")
    return match.group("version")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", help="release tag to validate, for example v0.0.1")
    parser.add_argument(
        "--skip-cli",
        action="store_true",
        help="skip building/running the CLI (metadata-only check)",
    )
    args = parser.parse_args()

    versions = workspace_versions(cargo_metadata())
    if not versions:
        raise SystemExit("no workspace crates found in cargo metadata")

    distinct = set(versions.values())
    if len(distinct) != 1:
        for name, version in sorted(versions.items()):
            print(f"  {name}: {version}")
        raise SystemExit("workspace crates have differing versions")
    version = distinct.pop()

    missing = PUBLISHABLE - set(versions)
    if missing:
        raise SystemExit(f"missing expected publishable crates: {sorted(missing)}")

    check_pyproject_dynamic_version()
    checked = [f"{len(versions)} crates", "pyproject dynamic version"]

    if not args.skip_cli:
        reported = cli_reported_version()
        if reported != version:
            raise SystemExit(
                f"CLI reports {reported} but the workspace version is {version}"
            )
        checked.append("CLI")

    if args.tag:
        tag_version = version_from_tag(args.tag)
        if tag_version != version:
            raise SystemExit(
                f"git tag {args.tag} does not match workspace version {version}"
            )
        checked.append(f"tag {args.tag}")

    print(f"OK: version {version} is consistent across {', '.join(checked)}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
