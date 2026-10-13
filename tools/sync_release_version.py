#!/usr/bin/env python3
"""Synchronize the Cargo workspace version with a release tag.

zarr-lint keeps a single authoritative version in the root ``Cargo.toml``
``[workspace.package]`` ``version`` field, and every crate inherits it with
``version.workspace = true``. That field is a *development placeholder* — it is
not expected to track the latest release. At release time the release tag (for
example ``v0.0.2``) is the source of truth: this script rewrites the workspace
``version`` and internal dependency pins to match the tag in the CI checkout,
updates workspace lockfile entries, and verifies the Python bindings crate
reports that version. Nothing is
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
import tomllib
from pathlib import Path

# The first standalone `version = "x.y.z"` line in the root Cargo.toml, which is
# the `[workspace.package]` version (every other `version` occurrence is nested
# inside a dependency entry on the same line as its key and so does not match).
VERSION_PATTERN = re.compile(r'^version = "\d+\.\d+\.\d+(?:[-+][^"]+)?"$', re.MULTILINE)
TAG_PATTERN = re.compile(r"^v?(?P<version>\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$")

# The Python bindings crate confirms the version maturin uses for PyPI.
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

    manifests = release_manifest_edits(version)

    if args.check:
        for path, text in manifests.items():
            if path.read_text() != text:
                raise SystemExit(
                    f"release version or dependency requirement differs in {path}"
                )
        package_version = cargo_package_version(REPRESENTATIVE_PACKAGE)
        if package_version != version:
            raise_version_mismatch(args.tag, package_version)
        print(f"tag={version} package={package_version}")
        return 0

    cargo_toml.write_text(next_text)
    for path, text in manifests.items():
        path.write_text(text)
    # Update workspace versions without upgrading unrelated dependencies.
    subprocess.run(["cargo", "update", "--workspace"], check=True)
    package_version = cargo_package_version(REPRESENTATIVE_PACKAGE)
    if package_version != version:
        raise_version_mismatch(args.tag, package_version)
    print(f"tag={version} package={package_version}")
    return 0


def release_manifest_edits(version: str) -> dict[Path, str]:
    """Plan exact internal dependency pins before modifying any manifests."""
    workspace = tomllib.loads(Path("Cargo.toml").read_text())["workspace"]
    manifests = [Path(member) / "Cargo.toml" for member in workspace["members"]]
    names = {tomllib.loads(path.read_text())["package"]["name"] for path in manifests}
    edits = {}
    for path in manifests:
        text = path.read_text()
        data = tomllib.loads(text)
        if data["package"].get("version") != {"workspace": True}:
            raise SystemExit(f"package in {path} must inherit the workspace version")
        for name, dependency in data.get("dependencies", {}).items():
            if (
                name not in names
                or not isinstance(dependency, dict)
                or "path" not in dependency
            ):
                continue
            if "version" not in dependency:
                raise SystemExit(
                    f"internal dependency {name} in {path} needs a version"
                )
            pattern = re.compile(
                rf'(^\s*{re.escape(name)}\s*=\s*\{{[^\n]*?\bversion\s*=\s*)"[^"]+"',
                re.MULTILINE,
            )
            text, count = pattern.subn(
                lambda match: f'{match.group(1)}"={version}"', text
            )
            if count != 1:
                raise SystemExit(f"could not rewrite dependency {name} in {path}")
        edits[path] = text
    return edits


def version_from_tag(tag: str) -> str:
    match = TAG_PATTERN.fullmatch(tag)
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
