# Test corpus

The `v0.0.1` corpus has two parts:

- **`test-data/upstream/duckdb-zarr/`** — a small, documented subset of real
  Zarr stores vendored from an upstream project. These are valid stores and are
  used as negative fixtures (they must produce no findings).
- **`test-data/invalid/`** — deliberately malformed derivatives, one per rule,
  used as positive fixtures.

## Provenance

Upstream fixtures were copied verbatim from the
[`WayScience/duckdb_zarr`][duckdb_zarr] repository (referred to as
`duckdb-zarr`), pinned to a single commit for reproducibility:

- **source-repository:** `WayScience/duckdb_zarr`
- **source-commit:** `c7e657edf261c04b5b5bdf9d1b6913906b6b3631`
- **source-path prefix:** `test/data/`

The `idr0062A` fixture from that repository was **not** vendored: it is a
multi-hundred-megabyte OME-Zarr image whose chunk binaries are irrelevant to a
metadata linter. The vendored stores already cover every structural case the
plan calls for. Do not modify the vendored upstream fixtures directly; add
derivatives under `test-data/invalid/` instead.

## Upstream fixtures

```yaml
- name: simple-v2
  source-repository: duckdb-zarr
  source-path: test/data/simple_v2.zarr
  source-commit: c7e657edf261c04b5b5bdf9d1b6913906b6b3631
  zarr-version: 2
  expected-result: valid
  covers:
    - valid Zarr v2 root group
    - valid Zarr v2 arrays (half_precision, mask, sparse_fill, temperature)
    - nested group and array (stations/, stations/elevation)
    - realistic 2x2 chunk layout (temperature: shape [4, 3], chunks [2, 2])

- name: simple-v3
  source-repository: duckdb-zarr
  source-path: test/data/simple_v3.zarr
  source-commit: c7e657edf261c04b5b5bdf9d1b6913906b6b3631
  zarr-version: 3
  expected-result: valid
  covers:
    - valid Zarr v3 root group
    - valid Zarr v3 arrays (fortran_v3, mask_v3, temperature_v3)
    - regular chunk grids and both default and v2 chunk-key encodings

- name: ome-example
  source-repository: duckdb-zarr
  source-path: test/data/ome_example.ome.zarr
  source-commit: c7e657edf261c04b5b5bdf9d1b6913906b6b3631
  zarr-version: 2
  expected-result: valid
  covers:
    - OME-Zarr v2 group with consolidated metadata (.zmetadata)
    - a 3-D array (shape [2, 2, 3], chunks [1, 2, 2])
```

## Invalid derivatives

Each derivative is a minimal, valid Zarr v2 group containing one deliberate
defect, so it triggers exactly one rule. They are modeled on `simple_v2.zarr`.

```yaml
- name: invalid-json
  path: test-data/invalid/invalid-json.zarr
  expected-result: error
  triggers: metadata/invalid-json
  defect: temperature/.zarray contains truncated (non-JSON) content

- name: missing-zarr-format
  path: test-data/invalid/missing-zarr-format.zarr
  expected-result: error
  triggers: metadata/missing-required-field
  defect: temperature/.zarray omits the zarr_format field

- name: unsupported-version
  path: test-data/invalid/unsupported-version.zarr
  expected-result: error
  triggers: metadata/unsupported-format-version
  defect: temperature/.zarray declares "zarr_format": 99

- name: rank-mismatch
  path: test-data/invalid/rank-mismatch.zarr
  expected-result: error
  triggers: array/rank-mismatch
  defect: temperature/.zarray has shape [4, 3] but chunks [2]

- name: conflicting-node-type
  path: test-data/invalid/conflicting-node-type.zarr
  expected-result: error
  triggers: structure/conflicting-node-type
  defect: temperature/ contains both .zgroup and .zarray

- name: not-a-zarr
  path: test-data/invalid/not-a-zarr
  expected-result: error
  triggers: structure/unrecognized-store
  defect: directory with a README and no Zarr metadata
```

## Generated ecosystem fixtures

To validate the experience against metadata written by tools people actually
use — not just one source — `test-data/generated/` holds small, **valid** stores
produced directly by popular Zarr writers. They are the "no false positives"
corpus: zarr-lint must report nothing for any of them. This is asserted by
[`real_ecosystem_stores_have_no_findings`](../crates/zarr-lint-cli/tests/cli.rs).

Regenerate with:

```bash
uv run --with zarr --with xarray --with tensorstore tools/generate_fixtures.py
```

Tool versions used at authoring time:

```yaml
- tool: zarr-python
  version: "3.3.0"
  stores: [zarr-python/v2.zarr, zarr-python/v3.zarr]
  note: reference implementation; v2 and v3, group + arrays + nested group

- tool: xarray
  version: "2026.7.0"        # writes via zarr-python; adds dimension metadata
  stores: [xarray/dataset_v2.zarr, xarray/dataset_v3.zarr]
  note: to_zarr Dataset; v2 uses consolidated metadata and _ARRAY_DIMENSIONS

- tool: tensorstore
  version: "0.1.84"          # independent C++ implementation
  stores: [tensorstore/array_v2.zarr, tensorstore/array_v3.zarr]
  note: single arrays via the zarr (v2) and zarr3 (v3) drivers

- support:
  numpy: "2.5.1"
```

These stores are committed complete (including their tiny chunk files) so that
Rust CI needs no Python toolchain to run the tests. Coverage can grow over time
(dask, n5/zarrita, ome-zarr-py) as the rule set expands.

## Refreshing the upstream fixtures

To re-vendor from a new upstream commit, update `source-commit` above and
re-copy the same relative paths under `test/data/` from the pinned commit. Keep
the set small; a metadata linter needs metadata documents and, at most, the tiny
example chunks that accompany them.

[duckdb_zarr]: https://github.com/WayScience/duckdb_zarr
