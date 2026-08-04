"""Tests for the zarr-lint Python package.

These require the compiled extension (``maturin develop`` or an installed
wheel). They exercise the public API and confirm the dynamically derived
version is consistent.
"""

from __future__ import annotations

from importlib.metadata import version
from pathlib import Path

import zarr_lint

REPO_ROOT = Path(__file__).resolve().parent.parent
UPSTREAM = REPO_ROOT / "test-data" / "upstream" / "duckdb-zarr"
INVALID = REPO_ROOT / "test-data" / "invalid"


def test_version_matches_distribution_metadata() -> None:
    assert zarr_lint.__version__ == version("zarr-lint")


def test_lint_valid_store_has_no_diagnostics() -> None:
    report = zarr_lint.lint(UPSTREAM / "simple_v2.zarr")
    assert report["diagnostics"] == []
    assert report["version"] == zarr_lint.__version__


def test_lint_invalid_store_reports_expected_rule() -> None:
    report = zarr_lint.lint(INVALID / "rank-mismatch.zarr")
    rules = [diagnostic["rule"] for diagnostic in report["diagnostics"]]
    assert rules == ["array/rank-mismatch"]


def test_rules_registry_lists_all_six_rules() -> None:
    rules = zarr_lint.rules()
    ids = {rule["id"] for rule in rules}
    assert len(rules) == 6
    assert "structure/unrecognized-store" in ids
    assert "array/rank-mismatch" in ids
