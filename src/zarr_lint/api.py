"""Python API for zarr-lint.

Thin wrappers over the native extension that return parsed Python objects. The
report schema matches the CLI's JSON output.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from zarr_lint import _native

#: The zarr-lint version (the single authoritative Cargo workspace version).
__version__: str = _native.version()


def lint(path: str | Path, *, anonymous: bool = False) -> dict[str, Any]:
    """Lint a Zarr store and return the report as a dictionary.

    ``path`` may be a local path, an ``http(s)://`` URL, or a cloud
    object-store URL (``s3://``, ``gs://``, ``az://``). Set ``anonymous`` for
    unauthenticated access to public cloud buckets. The returned mapping has
    ``version``, ``store``, and ``diagnostics`` keys, identical to the CLI's
    ``--format json`` output.
    """
    return json.loads(_native.lint_store_json(str(path), anonymous))


def rules() -> list[dict[str, Any]]:
    """Return the built-in rule registry as a list of dictionaries."""
    return json.loads(_native.rules_json())
