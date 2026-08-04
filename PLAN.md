# Revised Initial Implementation Plan

## Initial target: `v0.0.1`

The first release will be `v0.0.1`, not `v0.1.0`.

This version should establish the project architecture and prove that a Rust-based Zarr linter can recognize and inspect representative Zarr stores. It should be intentionally small and should not imply API stability or broad Zarr conformance coverage.

The `0.0.x` release line will be used for early architectural development:

* `v0.0.1`: repository bootstrap and minimal end-to-end linting
* `v0.0.2`: expanded structural checks and fixture coverage
* `v0.0.3`: consolidated metadata and improved reporting
* Later `v0.0.x` releases: chunk sampling, remote stores, and compatibility guidance
* `v0.1.0`: first intentionally usable public rule set with a more stable CLI and diagnostic model

## Dynamic versioning requirement

The project must follow the dynamic versioning pattern used by `arrow-lint`.

There should be one authoritative project version rather than separate manually maintained versions for:

* Rust crates
* The CLI
* Python packaging, if Python bindings are added
* `--version` output
* Build artifacts

### Version source of truth

The Cargo workspace version should initially be the canonical version:

```toml
[workspace.package]
version = "0.0.1"
edition = "2021"
license = "MIT"
repository = "https://github.com/ORG/PROJECT"
rust-version = "1.82"
```

Every workspace crate should inherit this version:

```toml
[package]
name = "zarr-lint-core"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true
```

The CLI crate should follow the same pattern:

```toml
[package]
name = "zarr-lint-cli"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true
```

This mirrors the existing `arrow-lint` workspace approach, where the workspace declares the version and individual crates use `version.workspace = true`.

### CLI version

The CLI should derive its version from Cargo at compile time:

```rust
#[derive(clap::Parser)]
#[command(
    name = "zarr-lint",
    version,
    about = "Inspect Zarr stores for structural and metadata problems."
)]
struct Cli {
    // ...
}
```

Alternatively, where an explicit value is needed:

```rust
const VERSION: &str = env!("CARGO_PKG_VERSION");
```

Do not hard-code `"0.0.1"` in Rust source files.

The following should therefore report the workspace package version:

```bash
zarr-lint --version
```

Expected initial result:

```text
zarr-lint 0.0.1
```

### Future Python package

Python bindings are not required for `v0.0.1`, but the repository should remain compatible with dynamic Python versioning.

When Python packaging is introduced, `pyproject.toml` should declare:

```toml
[project]
name = "zarr-lint"
dynamic = ["version"]
```

Maturin should derive the package version from the Rust crate rather than duplicating it in `pyproject.toml`. This follows the current `arrow-lint` packaging pattern.

### Release tags

Git tags should use the `v` prefix:

```text
v0.0.1
v0.0.2
v0.0.3
```

The package version remains:

```text
0.0.1
```

The release workflow must verify that the tag and Cargo workspace version agree.

For example:

```text
Git tag:                 v0.0.1
Cargo workspace version: 0.0.1
CLI version:             0.0.1
Python version:          0.0.1
```

A mismatch should fail the release workflow.

### Versioning verification tests

Add tests or release checks for:

```bash
cargo metadata --no-deps
cargo run --package zarr-lint-cli -- --version
```

The release pipeline should confirm:

1. All publishable crates resolve to the same workspace version.
2. The CLI reports the expected version.
3. The Git tag equals `v${CARGO_VERSION}`.
4. Any Python package reports the same version.
5. Documentation does not contain a separately maintained current-version field.

### Development builds

Local builds should remain identified by the Cargo version:

```text
0.0.1
```

Optional Git commit information can be exposed separately:

```bash
zarr-lint version --verbose
```

Example:

```text
zarr-lint 0.0.1
commit: a1b2c3d
build profile: release
```

The commit identifier should not replace or mutate the semantic package version unless a later release process explicitly adopts generated development versions.

## Scope of `v0.0.1`

`v0.0.1` should demonstrate one complete workflow:

```text
store path
    ↓
store discovery
    ↓
metadata loading
    ↓
normalized Zarr representation
    ↓
small rule set
    ↓
diagnostic report
    ↓
meaningful exit code
```

### Included in `v0.0.1`

* Rust Cargo workspace
* Dynamically inherited workspace version
* `zarr-lint` CLI
* Local filesystem stores
* Zarr v2 root and array recognition
* Zarr v3 root and array recognition
* JSON metadata parsing
* A minimal normalized node model
* A small rule registry
* Human-readable output
* JSON output
* Stable initial exit codes
* Representative fixtures from `duckdb-zarr`
* Deliberately malformed derivatives of those fixtures
* Unit and CLI integration tests
* CI for Rust formatting, linting, tests, and version consistency

### Explicitly excluded from `v0.0.1`

* Full specification conformance
* Chunk decoding
* Complete codec validation
* S3 support
* HTTP support
* Object-store credentials
* Python bindings
* SARIF output
* Automated repair
* User-defined rule plugins
* OME-Zarr-specific validation
* Xarray-specific validation
* Detailed performance recommendations
* Missing-chunk enumeration across large logical arrays
* Stable public Rust APIs

## Initial repository structure

The initial repository should remain smaller than the eventual architecture:

```text
zarr-lint/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── crates/
│   ├── zarr-lint-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── diagnostic.rs
│   │       ├── model.rs
│   │       ├── rule.rs
│   │       └── scanner.rs
│   │
│   └── zarr-lint-cli/
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
│
├── tests/
│   └── cli.rs
│
├── test-data/
│   ├── upstream/
│   │   └── duckdb-zarr/
│   └── invalid/
│
└── docs/
    ├── architecture.md
    ├── test-corpus.md
    └── versioning.md
```

Do not start with five or six crates unless real implementation pressure justifies them.

For `v0.0.1`, two crates are sufficient:

* `zarr-lint-core`
* `zarr-lint-cli`

Additional store, model, rule-pack, and Python crates can be extracted as the design matures.

## Minimal normalized model

The first internal model only needs to represent enough information for basic checks:

```rust
pub enum ZarrVersion {
    V2,
    V3,
}

pub enum NodeKind {
    Group,
    Array,
}

pub struct ZarrNode {
    pub path: PathBuf,
    pub version: ZarrVersion,
    pub kind: NodeKind,
    pub metadata_path: PathBuf,
    pub metadata: serde_json::Value,
}
```

This model can later be replaced with strongly typed Zarr v2 and v3 metadata structures.

For `v0.0.1`, preserving the raw JSON while extracting a few important fields is preferable to prematurely modeling the entire Zarr specification.

## Initial rules

Limit `v0.0.1` to approximately six rules.

### `structure/unrecognized-store`

The supplied path does not contain recognizable Zarr metadata.

### `metadata/invalid-json`

A recognized metadata file cannot be parsed as JSON.

### `metadata/missing-required-field`

A minimally required field is absent.

Examples include:

* Missing `zarr_format`
* Missing `node_type` in relevant Zarr v3 metadata
* Missing `shape` for an array
* Missing chunk shape or chunk-grid information

### `metadata/unsupported-format-version`

The metadata declares an unsupported Zarr format version.

### `structure/conflicting-node-type`

A path appears to contain conflicting array and group metadata.

### `array/rank-mismatch`

The dimensionality of the array shape and chunk shape does not agree.

These rules establish the architecture without claiming comprehensive validation.

## `duckdb-zarr` fixture integration

The `v0.0.1` test corpus should include a small, documented subset of `duckdb-zarr` fixtures.

Select fixtures covering:

* One valid Zarr v2 group
* One valid Zarr v2 array
* One valid Zarr v3 group
* One valid Zarr v3 array
* One nested group and array structure
* At least one realistic chunk layout

Copy fixtures into:

```text
test-data/upstream/duckdb-zarr/
```

Document each fixture in:

```text
docs/test-corpus.md
```

For every imported fixture, record:

```yaml
name: simple-v2-array
source-repository: duckdb-zarr
source-path: path/to/original
source-commit: COMMIT_SHA
zarr-version: 2
expected-result: valid
```

Create malformed derivatives under:

```text
test-data/invalid/
```

Initial mutations should include:

* Invalid metadata JSON
* Missing `zarr_format`
* Unsupported `zarr_format`
* Shape and chunk-rank mismatch
* Conflicting array and group markers
* Directory without recognizable metadata

Do not modify the imported upstream fixtures directly.

## CLI behavior for `v0.0.1`

Primary usage:

```bash
zarr-lint check path/to/store.zarr
```

A shorthand may also be supported:

```bash
zarr-lint path/to/store.zarr
```

Initial options:

```text
--format text|json
--fail-on warning|error|never
--quiet
--version
--help
```

Example text output:

```text
error[metadata/invalid-json] images/.zarray
  Metadata could not be parsed as JSON.

  Caused by:
    expected `,` at line 4 column 3
```

Example JSON output:

```json
{
  "version": "0.0.1",
  "store": "images.zarr",
  "diagnostics": [
    {
      "rule": "metadata/invalid-json",
      "severity": "error",
      "path": "images/.zarray",
      "message": "Metadata could not be parsed as JSON."
    }
  ]
}
```

The report version must be populated dynamically:

```rust
env!("CARGO_PKG_VERSION")
```

## Exit codes for `v0.0.1`

Use a small stable set:

```text
0  No findings reached the failure threshold
1  Lint findings reached the failure threshold
2  Invalid command usage or configuration
3  Store access or internal execution failure
```

More detailed exit codes can be introduced later, but existing meanings should not be changed casually.

## Implementation sequence

### Milestone 1: Workspace and dynamic versioning

Implement:

* Root Cargo workspace
* Workspace version `0.0.1`
* Crate inheritance through `version.workspace = true`
* CLI `--version`
* Version consistency test
* Initial CI
* `docs/versioning.md`

Completion criteria:

```bash
cargo run -p zarr-lint-cli -- --version
```

reports:

```text
zarr-lint 0.0.1
```

No source file contains a separately hard-coded package version.

### Milestone 2: Store discovery

Implement:

* Local path validation
* Recursive metadata-file discovery
* Recognition of Zarr v2 metadata names
* Recognition of Zarr v3 metadata
* Basic path normalization
* Store-access diagnostics

Completion criteria:

The scanner identifies the selected valid `duckdb-zarr` fixtures as candidate Zarr stores.

### Milestone 3: Minimal metadata model

Implement:

* JSON parsing
* Zarr version extraction
* Node-type extraction
* Array shape extraction
* Chunk-shape extraction
* Normalized node records

Completion criteria:

The tool can print a basic internal summary of groups and arrays from both Zarr versions.

### Milestone 4: Initial rules

Implement the six `v0.0.1` rules:

```text
structure/unrecognized-store
metadata/invalid-json
metadata/missing-required-field
metadata/unsupported-format-version
structure/conflicting-node-type
array/rank-mismatch
```

Completion criteria:

Each rule has:

* One fixture that produces the diagnostic
* One fixture that does not
* A stable rule identifier
* A documented default severity

### Milestone 5: Reporting and CLI integration

Implement:

* Text reports
* JSON reports
* Deterministic diagnostic ordering
* Failure thresholds
* Exit-code integration
* Version field in JSON output

Completion criteria:

The CLI is suitable for use in a basic CI workflow.

### Milestone 6: Release preparation

Implement:

* Release workflow
* Tag-to-Cargo-version validation
* Binary builds
* Checksums
* Release notes
* Installation instructions
* Clean clone installation test

Completion criteria:

A Git tag named `v0.0.1` produces artifacts whose embedded version is `0.0.1`.

## Definition of done for `v0.0.1`

The initial release is complete when:

* The Cargo workspace version is `0.0.1`.
* All crates inherit the workspace version dynamically.
* The CLI reports version `0.0.1`.
* Release automation verifies that tag `v0.0.1` matches Cargo version `0.0.1`.
* No duplicate package version is maintained in source code or packaging configuration.
* The CLI recognizes selected Zarr v2 and v3 fixtures from `duckdb-zarr`.
* The six initial rules are implemented and documented.
* Every rule has positive and negative tests.
* Invalid metadata does not panic the application.
* Text and JSON outputs are available.
* Diagnostic ordering is deterministic.
* Exit codes are documented and tested.
* The test corpus records source repository and commit provenance.
* CI runs formatting, Clippy, tests, and version checks.
* Release binaries are produced for the initially supported platforms.
* Unsupported capabilities are clearly documented.
* The project makes no claim of complete Zarr specification validation.

## Work immediately after `v0.0.1`

The likely `v0.0.2` scope should be:

* Complete local store inventory
* Unexpected object detection
* Basic chunk-key parsing
* Out-of-bounds chunk-coordinate checks
* Additional `duckdb-zarr` regression fixtures
* Rule selection and severity configuration
* Improved JSON report schema

The likely `v0.0.3` scope should be:

* Consolidated metadata checks
* Stale and missing consolidated entries
* Traversal limits
* Improved compatibility diagnostics
* Early consideration of remote-store abstractions

Chunk decoding, remote object stores, and domain-specific profiles should remain later `0.0.x` work unless a concrete use case requires them sooner.
