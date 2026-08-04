# Architecture

zarr-lint runs one clear workflow — discover, parse, lint, report — over a small,
focused set of crates. The design favors a simple, well-understood core that is
easy to extend rather than a broad model of the entire Zarr specification.

## Pipeline

```text
store path
    ↓  scanner::scan_store        (discovery: find metadata files by name)
store scan
    ↓  model::load                (parse each metadata file as JSON)
loaded store (nodes + parse failures)
    ↓  rule::evaluate             (run the rule set)
diagnostics
    ↓  lint_store sorts them      (deterministic ordering)
Report
    ↓  CLI reporters              (text or JSON) + exit code
```

The single public entry point is
[`zarr_lint_core::lint_store`](../crates/zarr-lint-core/src/lib.rs), which
returns a serializable [`Report`].

## Crates

The core linting engine lives in two crates; a thin third crate provides the
Python bindings. Additional store and rule-pack crates can be extracted later
when real implementation pressure justifies them.

| Crate              | Responsibility                                            |
| ------------------ | -------------------------------------------------------- |
| `zarr-lint-core`   | Discovery, parsing, the normalized model, and the rules. |
| `zarr-lint-cli`    | The CLI, exposed as a library (`run`) plus a thin binary. |
| `zarr-lint-python` | pyo3 bindings: a `_native` module wrapping the above.     |

The CLI logic lives in the `zarr-lint-cli` **library**, so the native binary and
the Python console script (`zarr-lint`) run byte-for-byte the same code — the
Python entry point simply calls `zarr_lint_cli::run` through pyo3. The bindings
crate is a pyo3 extension module (`crate-type = ["cdylib"]`, `abi3-py311`) and is
built with maturin; it is excluded from the Cargo `default-members` because a
plain `cargo build` cannot link an extension module.

### `zarr-lint-core` modules

- `scanner` — walks a local directory and collects recognized metadata files
  (`.zgroup`, `.zarray`, `zarr.json`) by name. It does no parsing. Store-access
  problems (missing path, not a directory, unreadable file) surface as
  `ScanError`.
- `model` — parses each discovered file into a raw `serde_json::Value`, extracts
  a few key fields (`zarr_format`, `node_type`, shape, chunk shape), and offers
  the normalized [`ZarrNode`] type. Files that fail to parse become
  `ParseFailure`s rather than aborting the load.
- `diagnostic` — the `Severity` and `Diagnostic` types shared by the rules and
  the reporters.
- `rule` — the rules and their registry (`RULES`).

## The normalized model

The first model preserves the raw JSON and extracts only what the initial rules
need:

```rust
pub enum ZarrVersion { V2, V3 }
pub enum NodeKind { Group, Array }

pub struct ZarrNode {
    pub path: PathBuf,          // node directory
    pub logical_path: String,   // store-relative, "" at the root
    pub version: ZarrVersion,
    pub kind: NodeKind,
    pub metadata_path: PathBuf, // the metadata document
    pub metadata: serde_json::Value,
}
```

Version is inferred from the file name (`.zgroup`/`.zarray` → v2, `zarr.json` →
v3). Node kind is the file name for v2 and the `node_type` field for v3. This
deliberately shallow model can later be replaced with strongly typed v2 and v3
metadata structures.

## Rules

See [rules.md](rules.md) for the full list, default severities, and the fixtures
that exercise each rule.

## Exit codes

| Code | Meaning                                            |
| ---- | -------------------------------------------------- |
| `0`  | No findings reached the failure threshold.         |
| `1`  | Lint findings reached the failure threshold.       |
| `2`  | Invalid command usage or configuration.            |
| `3`  | Store access or internal execution failure.        |

These meanings are stable. More detailed codes may be added later, but existing
meanings will not change casually.

## Determinism

Findings are sorted by `(path, rule, message)` before reporting, so a given
store always produces byte-identical output regardless of filesystem traversal
order. Directory traversal is itself sorted by file name.

## Design boundaries

zarr-lint inspects store **metadata**; it does not read or decode chunk data, and
it is not a full Zarr specification validator. It runs a focused, dependable rule
set, and broader capabilities — remote object stores, codec validation, and
domain-specific profiles such as OME-Zarr and Xarray — build on this same core.

[`Report`]: ../crates/zarr-lint-core/src/lib.rs
[`ZarrNode`]: ../crates/zarr-lint-core/src/model.rs
