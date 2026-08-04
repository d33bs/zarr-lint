# Rules

Each rule has a stable identifier, a documented default severity, and both a
positive fixture (which triggers it) and a negative fixture (which does not). The
registry lives in
[`crates/zarr-lint-core/src/rule.rs`](../crates/zarr-lint-core/src/rule.rs).

Every rule defaults to **error** severity. The `--fail-on` flag selects the
severity at or above which findings cause a non-zero exit (`error` by default;
also `warning` or `never`).

| Rule                                   | Default | What it detects                                                        |
| -------------------------------------- | ------- | --------------------------------------------------------------------- |
| `structure/unrecognized-store`         | error   | The supplied path contains no recognizable Zarr metadata.             |
| `metadata/invalid-json`                | error   | A recognized metadata file cannot be parsed as JSON.                  |
| `metadata/missing-required-field`      | error   | A minimally required field is absent.                                 |
| `metadata/unsupported-format-version`  | error   | `zarr_format` is present but is not `2` or `3`.                       |
| `structure/conflicting-node-type`      | error   | One path declares both array and group (or both v2 and v3) metadata.  |
| `array/rank-mismatch`                  | error   | An array's shape rank differs from its chunk-shape rank.             |

## Fixture coverage

Positive fixtures are the malformed derivatives under `test-data/invalid/`;
negative fixtures are the valid upstream stores under
`test-data/upstream/duckdb-zarr/`. See [test-corpus.md](test-corpus.md).

| Rule                                  | Positive fixture                          | Negative fixture         |
| ------------------------------------- | ----------------------------------------- | ------------------------ |
| `structure/unrecognized-store`        | `invalid/not-a-zarr`                       | `.../simple_v2.zarr`     |
| `metadata/invalid-json`               | `invalid/invalid-json.zarr`               | `.../simple_v2.zarr`     |
| `metadata/missing-required-field`     | `invalid/missing-zarr-format.zarr`        | `.../simple_v2.zarr`     |
| `metadata/unsupported-format-version` | `invalid/unsupported-version.zarr`        | `.../simple_v2.zarr`     |
| `structure/conflicting-node-type`     | `invalid/conflicting-node-type.zarr`      | `.../simple_v2.zarr`     |
| `array/rank-mismatch`                 | `invalid/rank-mismatch.zarr`              | `.../simple_v3.zarr`     |

The mapping is asserted end to end by
[`each_invalid_fixture_triggers_its_rule`](../crates/zarr-lint-cli/tests/cli.rs).

## Rule details

### `structure/unrecognized-store`

Fires once, at the store level, when discovery finds no `.zgroup`, `.zarray`, or
`zarr.json` anywhere under the supplied path. When it fires no other rule runs,
because there are no nodes to inspect.

### `metadata/invalid-json`

Fires per file whose bytes are not valid JSON. The underlying parser error is
included as `Caused by:` detail. A file that fails to parse is not subjected to
the field-based rules below.

### `metadata/missing-required-field`

Fires once per missing field. Required fields:

- All nodes: `zarr_format`.
- Zarr v3 nodes: `node_type`.
- Array nodes: `shape`, and the chunk grid (`chunks` in v2, `chunk_grid` in v3).

For a v3 document with no usable `node_type`, only the missing `node_type` is
reported; array-specific fields are not checked because the node's kind is
unknown.

### `metadata/unsupported-format-version`

Fires when `zarr_format` is present and its integer value is neither `2` nor
`3`. A *missing* `zarr_format` is reported by `metadata/missing-required-field`
instead.

### `structure/conflicting-node-type`

Fires per location (directory) that declares both group and array metadata, or
that mixes Zarr v2 and Zarr v3 markers. The offending marker files are listed as
`Caused by:` detail.

### `array/rank-mismatch`

Fires per array whose shape rank and chunk-shape rank differ, for example a 2-D
`shape` with a 1-D `chunks`. Both dimensions are shown as detail. Ranks are only
compared when both are determinable.
