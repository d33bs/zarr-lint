"""Regression tests for tag-driven Cargo and Python release versions."""

from __future__ import annotations

import runpy
import shutil
import subprocess
import json
import sys
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SYNC = runpy.run_path(str(ROOT / "tools" / "sync_release_version.py"))


def test_one_public_crate_provides_library_and_binary() -> None:
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=ROOT,
            text=True,
        )
    )
    packages = {package["name"]: package for package in metadata["packages"]}
    public = {name for name, package in packages.items() if package["publish"] != []}
    assert public == {"zarr-lint"}
    assert set(packages) == {"zarr-lint", "zarr-lint-python"}
    targets = {
        (target["name"], tuple(target["kind"]))
        for target in packages["zarr-lint"]["targets"]
    }
    assert ("zarr_lint", ("lib",)) in targets
    assert ("zarr-lint", ("bin",)) in targets
    assert metadata["workspace_default_members"] == [packages["zarr-lint"]["id"]]
    dependencies = packages["zarr-lint-python"]["dependencies"]
    assert [dep["name"] for dep in dependencies if dep.get("path")] == ["zarr-lint"]
    assert all(dep["name"] != "pyo3" for dep in packages["zarr-lint"]["dependencies"])


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


def test_release_rejects_literal_package_versions(checkout: Path) -> None:
    path = checkout / "crates" / "zarr-lint-python" / "Cargo.toml"
    path.write_text(
        path.read_text().replace(
            "\nversion.workspace = true\n", '\nversion = "0.0.1"\n'
        )
    )
    with pytest.raises(SystemExit, match="inherit the workspace version"):
        SYNC["release_manifest_edits"]("1.2.3")


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
        assert path.read_text() != text or data["package"]["name"] == "zarr-lint"
    assert dependencies == ["zarr-lint"]


def test_missing_dependency_version_fails_before_writing(checkout: Path) -> None:
    path = checkout / "crates" / "zarr-lint-python" / "Cargo.toml"
    path.write_text(path.read_text().replace(', version = "=0.0.1"', ""))
    original = (checkout / "Cargo.toml").read_text()
    with pytest.raises(SystemExit, match="needs a version"):
        SYNC["release_manifest_edits"]("1.2.3")
    assert (checkout / "Cargo.toml").read_text() == original


def test_check_detects_dependency_drift(
    checkout: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(sys, "argv", ["sync_release_version.py", "v0.0.1", "--check"])
    path = checkout / "crates" / "zarr-lint-python" / "Cargo.toml"
    path.write_text(
        path.read_text().replace('version = "=0.0.1"', 'version = "=0.0.2"')
    )
    before = path.read_text()
    with pytest.raises(SystemExit, match="dependency requirement differs"):
        SYNC["main"]()
    assert path.read_text() == before
