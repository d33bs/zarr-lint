"""Regression tests for tag-driven Cargo and Python release versions."""

from __future__ import annotations

import runpy
import shutil
import sys
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SYNC = runpy.run_path(str(ROOT / "tools" / "sync_release_version.py"))


@pytest.fixture
def checkout(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    shutil.copy(ROOT / "Cargo.toml", tmp_path / "Cargo.toml")
    shutil.copytree(ROOT / "crates", tmp_path / "crates")
    monkeypatch.chdir(tmp_path)
    return tmp_path


@pytest.mark.parametrize("tag", ["v1.2.3", "1.2.3", "v1.2.3-rc.1"])
def test_tag_version(tag: str) -> None:
    assert SYNC["version_from_tag"](tag) == tag.removeprefix("v")


@pytest.mark.parametrize("tag", ["main", "v1.2", "v1.2.3/evil", "v1.2.3\n"])
def test_invalid_tag(tag: str) -> None:
    with pytest.raises(SystemExit):
        SYNC["version_from_tag"](tag)


def test_release_pins_every_internal_dependency(checkout: Path) -> None:
    edits = SYNC["release_manifest_edits"]("1.2.3")
    dependencies = []
    for path, text in edits.items():
        data = tomllib.loads(text)
        for name, dependency in data.get("dependencies", {}).items():
            if isinstance(dependency, dict) and "path" in dependency:
                assert dependency["version"] == "=1.2.3"
                dependencies.append(name)
        # Planning must not write to the checkout.
        assert path.read_text() != text or data["package"]["name"] == "zarr-lint-core"
    assert sorted(dependencies) == ["zarr-lint-cli", "zarr-lint-core", "zarr-lint-core"]


def test_missing_dependency_version_fails_before_writing(checkout: Path) -> None:
    path = checkout / "crates" / "zarr-lint-cli" / "Cargo.toml"
    path.write_text(path.read_text().replace(', version = "=0.0.1"', ""))
    original = (checkout / "Cargo.toml").read_text()
    with pytest.raises(SystemExit, match="needs a version"):
        SYNC["release_manifest_edits"]("1.2.3")
    assert (checkout / "Cargo.toml").read_text() == original


def test_check_detects_dependency_drift(
    checkout: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(sys, "argv", ["sync_release_version.py", "v0.0.1", "--check"])
    path = checkout / "crates" / "zarr-lint-cli" / "Cargo.toml"
    path.write_text(
        path.read_text().replace('version = "=0.0.1"', 'version = "=0.0.2"')
    )
    before = path.read_text()
    with pytest.raises(SystemExit, match="dependency requirement differs"):
        SYNC["main"]()
    assert path.read_text() == before
