<img src="https://raw.githubusercontent.com/d33bs/zarr-lint/main/docs/assets/zarr-lint-logo.png" alt="zarr-lint" width="140">

# zarr-lint

Inspect, validate, and understand Zarr stores with fast structural, metadata,
and compatibility checks. Built for scientific data and reproducible workflows.

## What it does

`zarr-lint` points at a local Zarr store, recognizes Zarr v2 and v3 metadata,
and reports structural and metadata problems as diagnostics — in human-readable
text or JSON, with stable exit codes suitable for CI.

```console
$ zarr-lint check images.zarr
error[array/rank-mismatch] temperature/.zarray
  Array shape rank (2) does not match chunk shape rank (1).

  Caused by:
    shape [4, 3], chunks [2]

1 finding(s): 1 error(s), 0 warning(s), 0 info.
```

## Install

From PyPI (installs the `zarr-lint` command and a Python API):

```bash
pip install zarr-lint
```

From source with Cargo (requires Rust 1.82+):

```bash
cargo install --path crates/zarr-lint-cli
```

Or build the binary in-tree:

```bash
cargo build --release
# binary at target/release/zarr-lint
```

## Usage

```text
zarr-lint check <PATH>     Check a store (primary form)
zarr-lint <PATH>           Shorthand for `check`
zarr-lint inspect <PATH>   Print a summary of discovered groups and arrays
zarr-lint version          Print version (add --verbose for commit/profile)
```

Options for `check`:

```text
--format text|json         Output format (default: text)
--fail-on warning|error|never
                           Severity at/above which findings fail (default: error)
--quiet                    Suppress the summary/success line (text output)
```

### Examples

```bash
# Human-readable check
zarr-lint check path/to/store.zarr

# JSON for machines / CI
zarr-lint check --format json path/to/store.zarr

# Report problems but never fail the build
zarr-lint check --fail-on never path/to/store.zarr

# See what the linter discovered
zarr-lint inspect path/to/store.zarr
```

JSON output:

```json
{
  "version": "0.0.1",
  "store": "images.zarr",
  "diagnostics": [
    {
      "rule": "array/rank-mismatch",
      "severity": "error",
      "path": "temperature/.zarray",
      "message": "Array shape rank (2) does not match chunk shape rank (1).",
      "detail": "shape [4, 3], chunks [2]"
    }
  ]
}
```

## Python API

The PyPI package ships the same `zarr-lint` command plus a small Python API. The
report matches the CLI's JSON schema:

```python
import zarr_lint

zarr_lint.__version__          # "0.0.1"
report = zarr_lint.lint("images.zarr")
report["diagnostics"]          # list of {rule, severity, path, message, ...}
zarr_lint.rules()              # the built-in rule registry
```

## Rules

zarr-lint checks the following rules, each reporting at `error` severity:

| Rule                                   | What it detects                                          |
| -------------------------------------- | ------------------------------------------------------- |
| `structure/unrecognized-store`         | No recognizable Zarr metadata under the path.           |
| `metadata/invalid-json`                | A metadata file is not valid JSON.                      |
| `metadata/missing-required-field`      | A required field is absent.                              |
| `metadata/unsupported-format-version`  | `zarr_format` is not `2` or `3`.                        |
| `structure/conflicting-node-type`      | A path declares both array and group metadata.          |
| `array/rank-mismatch`                  | Shape rank differs from chunk-shape rank.               |

Details and per-rule fixtures: [docs/rules.md](docs/rules.md).

## Exit codes

| Code | Meaning                                        |
| ---- | ---------------------------------------------- |
| `0`  | No findings reached the failure threshold.     |
| `1`  | Findings reached the failure threshold.        |
| `2`  | Invalid command usage or configuration.        |
| `3`  | Store access or internal execution failure.    |

## Scope

zarr-lint reads store **metadata** — the `.zgroup`, `.zarray`, and `zarr.json`
documents of Zarr v2 and v3 stores — and reports structural and metadata
problems. It inspects metadata rather than reading or decoding chunk data, which
keeps checks fast and lightweight. Coverage grows with each release; see the
[rules](docs/rules.md).

Stores can be **local** (a filesystem path) or **remote** over `http(s)://`,
including public object stores reached through their HTTPS endpoints. Because
HTTP has no directory listing, remote discovery uses the store's consolidated
metadata (`.zmetadata`) when present, and otherwise reads the root node.

```bash
zarr-lint check path/to/store.zarr
zarr-lint check https://example.com/data.zarr
zarr-lint check https://bucket.s3.amazonaws.com/prefix/data.zarr
```

Native `s3://` (and `gs://`, `azure://`) access with credentials is not yet
supported; use the equivalent `https://` URL for public data.

## Development

```bash
cargo test --workspace          # unit + CLI integration tests
cargo fmt --all --check         # formatting
cargo clippy --all-targets --all-features -- -D warnings
python tools/check_versions.py  # version consistency
prek run --all-files            # all pre-commit hooks (or: pre-commit run)
```

The test corpus includes small, real stores written by zarr-python, xarray, and
tensorstore (under `test-data/generated/`) to guard against false positives on
mainstream tools. Regenerate them with:

```bash
uv run --with zarr --with xarray --with tensorstore tools/generate_fixtures.py
```

Documentation:

- [docs/architecture.md](docs/architecture.md) — pipeline, crates, and model.
- [docs/rules.md](docs/rules.md) — the rule set and fixtures.
- [docs/versioning.md](docs/versioning.md) — the dynamic versioning model.
- [docs/test-corpus.md](docs/test-corpus.md) — fixture provenance.

## License

BSD 3-Clause. See [LICENSE](LICENSE).
