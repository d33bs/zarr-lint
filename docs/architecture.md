# Architecture

This document describes the `v0.0.1` architecture. It is intentionally small:
the goal of the first release is to prove one complete workflow end to end, not
to model the whole Zarr specification.

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

For `v0.0.1` two crates are sufficient. Additional store, rule-pack, and Python
crates can be extracted later when real implementation pressure justifies them.

| Crate            | Responsibility                                             |
| ---------------- | ---------------------------------------------------------- |
| `zarr-lint-core` | Discovery, parsing, the normalized model, and the rules.   |
| `zarr-lint-cli`  | Argument parsing, text/JSON reporting, and exit codes.     |

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
- `rule` — the six `v0.0.1` rules and their registry (`RULES`).

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

## Explicit non-goals for `v0.0.1`

Full specification conformance, chunk decoding, complete codec validation, S3
and HTTP stores, Python bindings, SARIF output, automated repair, user-defined
rule plugins, and domain-specific (OME-Zarr, Xarray) validation are all out of
scope. `zarr-lint` makes **no claim** of complete Zarr specification validation.

[`Report`]: ../crates/zarr-lint-core/src/lib.rs
[`ZarrNode`]: ../crates/zarr-lint-core/src/model.rs
