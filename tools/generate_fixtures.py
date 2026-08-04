#!/usr/bin/env python3
"""Generate small, real Zarr stores from popular ecosystem tools.

These fixtures exist so zarr-lint can be validated against metadata written by
the tools people actually use, not just hand-written or single-source examples.
Every store here is expected to be **valid** (zarr-lint should report no
findings); they are the ecosystem "no false positives" corpus.

Reproduce with::

    uv run --with zarr --with xarray --with tensorstore \
        tools/generate_fixtures.py

Each generator degrades gracefully: if a tool is not importable it is skipped
with a printed note, so a partial toolchain still refreshes what it can. Arrays
are intentionally tiny.

Tool versions used at authoring time are recorded in docs/test-corpus.md.
"""

from __future__ import annotations

import shutil
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
DEST_ROOT = REPO_ROOT / "test-data" / "generated"


def reset(*parts: str) -> Path:
    path = DEST_ROOT.joinpath(*parts)
    if path.exists():
        shutil.rmtree(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def rel(path: Path) -> str:
    return str(path.relative_to(REPO_ROOT))


def gen_zarr_python(fmt: int) -> None:
    """A group + array + nested group written by zarr-python."""
    import numpy as np
    import zarr

    dest = reset("zarr-python", f"v{fmt}.zarr")
    group = zarr.open_group(str(dest), mode="w", zarr_format=fmt)
    group.attrs["description"] = f"zarr-python v{fmt} fixture"

    temperature = group.create_array(
        "temperature", shape=(4, 3), chunks=(2, 2), dtype="float64"
    )
    temperature[:] = np.arange(12, dtype="float64").reshape(4, 3)
    temperature.attrs["units"] = "celsius"

    stations = group.create_group("stations")
    elevation = stations.create_array(
        "elevation", shape=(5,), chunks=(2,), dtype="int32"
    )
    elevation[:] = np.arange(5, dtype="int32")
    print(f"  zarr-python v{fmt}: {rel(dest)}")


def gen_xarray(fmt: int, consolidated: bool) -> None:
    """A typical xarray Dataset written with to_zarr (adds dimension metadata)."""
    import numpy as np
    import xarray as xr

    dest = reset("xarray", f"dataset_v{fmt}.zarr")
    ds = xr.Dataset(
        {
            "temperature": (("y", "x"), np.arange(12, dtype="f8").reshape(4, 3)),
            "mask": (("y", "x"), (np.arange(12).reshape(4, 3) % 2).astype(bool)),
        },
        coords={"x": [0, 1, 2], "y": [0, 1, 2, 3]},
        attrs={"title": "xarray fixture"},
    )
    ds.to_zarr(str(dest), mode="w", zarr_format=fmt, consolidated=consolidated)
    tag = "consolidated" if consolidated else "plain"
    print(f"  xarray v{fmt} ({tag}): {rel(dest)}")


def gen_tensorstore(fmt: int) -> None:
    """A single array written by tensorstore (an independent implementation)."""
    import numpy as np
    import tensorstore as ts

    driver = "zarr" if fmt == 2 else "zarr3"
    dest = reset("tensorstore", f"array_v{fmt}.zarr")
    if fmt == 2:
        metadata = {"shape": [4, 3], "chunks": [2, 2], "dtype": "<f8"}
    else:
        metadata = {
            "shape": [4, 3],
            "chunk_grid": {"name": "regular", "configuration": {"chunk_shape": [2, 2]}},
            "data_type": "float64",
        }
    store = ts.open(
        {
            "driver": driver,
            "kvstore": {"driver": "file", "path": str(dest)},
            "metadata": metadata,
            "create": True,
            "delete_existing": True,
        }
    ).result()
    store[...] = np.arange(12, dtype="f8").reshape(4, 3)
    print(f"  tensorstore v{fmt} ({driver}): {rel(dest)}")


def run(label: str, fn, *args) -> bool:
    try:
        fn(*args)
        return True
    except ImportError as exc:
        print(f"  [skip] {label}: {exc.name} not installed")
    except Exception as exc:
        print(f"  [FAIL] {label}: {type(exc).__name__}: {exc}")
    return False


def main() -> int:
    print("Generating real ecosystem fixtures under", rel(DEST_ROOT))
    generators = [
        ("zarr-python v2", gen_zarr_python, 2),
        ("zarr-python v3", gen_zarr_python, 3),
        ("xarray v2 consolidated", gen_xarray, 2, True),
        ("xarray v3", gen_xarray, 3, False),
        ("tensorstore v2", gen_tensorstore, 2),
        ("tensorstore v3", gen_tensorstore, 3),
    ]
    made = 0
    for label, fn, *args in generators:
        if run(label, fn, *args):
            made += 1
    print(f"Done: {made}/{len(generators)} fixtures generated.")
    # Non-zero only if nothing at all could be produced.
    return 0 if made else 1


if __name__ == "__main__":
    raise SystemExit(main())
