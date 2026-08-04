"""Console entry point for the ``zarr-lint`` command.

This delegates to the native extension, which runs the exact same command line
implementation as the standalone Rust binary, so behavior and exit codes are
identical whether installed from PyPI or from crates.io.
"""

from __future__ import annotations

import sys

from zarr_lint import _native


def main() -> None:
    """Run the zarr-lint CLI and exit with its status code."""
    raise SystemExit(_native.run_cli(sys.argv))
