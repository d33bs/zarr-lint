"""zarr-lint: inspect Zarr stores for structural and metadata problems."""

from zarr_lint.api import __version__, lint, rules

__all__ = ["__version__", "lint", "rules"]
