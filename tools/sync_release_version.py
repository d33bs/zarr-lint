#!/usr/bin/env python3
"""Synchronize the Cargo workspace version with a release tag.

zarr-lint keeps a single authoritative version in the root ``Cargo.toml``
``[workspace.package]`` ``version`` field, and every crate inherits it with
``version.workspace = true``. That field is a *development placeholder* — it is
not expected to track the latest release. At release time the release tag (for
example ``v0.0.2``) is the source of truth: this script rewrites the workspace
``version`` to match the tag in the CI checkout, regenerates the lockfile, and
verifies a representative publishable crate reports that version. Nothing is
committed; the rewrite lives only in the ephemeral build checkout so the
published artifacts carry the tag version while the repository keeps its
development placeholder.

This mirrors the dynamic-versioning approach used by the sibling
[`arrow-lint`](https://github.com/d33bs/arrow-lint) project.

Usage::

    python tools/sync_release_version.py v0.0.2        # rewrite + verify
    python tools/sync_release_version.py v0.0.2 --check  # verify only, no edit
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path

# The first standalone `version = "x.y.z"` line in the root Cargo.toml, which is
# the `[workspace.package]` version (every other `version` occurrence is nested
# inside a dependency entry on the same line as its key and so does not match).
VERSION_PATTERN = re.compile(r'^version = "\d+\.\d+\.\d+(?:[-+][^"]+)?"$', re.MULTILINE)
TAG_PATTERN = re.compile(r"^v?(?P<version>\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$")

# A publishable workspace crate used to confirm the inherited version took hold.
REPRESENTATIVE_PACKAGE = "zarr-lint-python"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag", help="release tag, for example v0.0.2")
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify the tag against the current package version without editing",
    )
    args = parser.parse_args()

    version = version_from_tag(args.tag)
    cargo_toml = Path("Cargo.toml")
    current_text = cargo_toml.read_text()
    next_text = VERSION_PATTERN.sub(f'version = "{version}"', current_text, count=1)
    if current_text == next_text and f'version = "{version}"' not in current_text:
        raise SystemExit("could not find workspace package version in Cargo.toml")

    if args.check:
        package_version = cargo_package_version(REPRESENTATIVE_PACKAGE)
        if package_version != version:
            raise_version_mismatch(args.tag, package_version)
        print(f"tag={version} package={package_version}")
        return 0

    cargo_toml.write_text(next_text)
    subprocess.run(["cargo", "generate-lockfile"], check=True)
    package_version = cargo_package_version(REPRESENTATIVE_PACKAGE)
    if package_version != version:
        raise_version_mismatch(args.tag, package_version)
    print(f"tag={version} package={package_version}")
    return 0


def version_from_tag(tag: str) -> str:
    match = TAG_PATTERN.match(tag)
    if not match:
        raise SystemExit(f"release tag must look like v1.2.3: {tag}")
    return match.group("version")


def cargo_package_version(package_name: str) -> str:
    output = subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        text=True,
    )
    metadata = json.loads(output)
    for package in metadata["packages"]:
        if package["name"] == package_name:
            return str(package["version"])
    raise SystemExit(f"could not find Cargo package {package_name}")


def raise_version_mismatch(tag: str, package_version: str) -> None:
    raise SystemExit(
        f"release tag {tag} does not match package version {package_version}"
    )


if __name__ == "__main__":
    raise SystemExit(main())
