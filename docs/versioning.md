# Versioning

`zarr-lint` follows a **dynamic versioning** model: there is exactly one place
the project version is written, and everything else derives from it. This
mirrors the approach used by the sibling [`arrow-lint`][arrow-lint] project.

## Source of truth

The canonical version is the Cargo **workspace** package version in the root
[`Cargo.toml`](../Cargo.toml):

```toml
[workspace.package]
version = "0.0.1"
```

Every crate inherits it rather than declaring its own:

```toml
[package]
name = "zarr-lint-core"
version.workspace = true
```

The CLI reads it from Cargo at compile time; it is never written into Rust
source:

```rust
// crates/zarr-lint-core/src/lib.rs
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

As a result, the following all report the same value:

```console
$ zarr-lint --version
zarr-lint 0.0.1
```

```json
{ "version": "0.0.1", "store": "images.zarr", "diagnostics": [] }
```

## What must never happen

- No crate declares a literal `version = "0.0.1"` of its own.
- No Rust source file hard-codes the version; use `env!("CARGO_PKG_VERSION")`.
- No documentation maintains a separate "current version" field that could
  drift. (Version strings shown in examples here are illustrative.)

## Release tags

Release tags use a `v` prefix; the package version omits it:

| Git tag  | Package version |
| -------- | --------------- |
| `v0.0.1` | `0.0.1`         |

The release workflow refuses to publish if the tag and the workspace version
disagree.

## Development builds

Local builds are identified by the Cargo version. Optional git information is
available separately and never mutates the semantic version:

```console
$ zarr-lint version --verbose
zarr-lint 0.0.1
commit: a1b2c3d
build profile: debug
```

When git is unavailable (for example, building from a source tarball) the commit
is reported as `unknown`.

## Verifying consistency

[`tools/check_versions.py`](../tools/check_versions.py) enforces the invariant.
It is run in CI on every push and, with `--tag`, as a release gate.

```console
$ python tools/check_versions.py
OK: version 0.0.1 is consistent across 2 crates, CLI.

$ python tools/check_versions.py --tag v0.0.1
OK: version 0.0.1 is consistent across 2 crates, CLI, tag v0.0.1.
```

It checks that:

1. Every workspace crate resolves to the same version.
2. The crates expected to be published are present.
3. The CLI reports that version.
4. A supplied release tag equals `v<version>`.

## Future Python packaging

Python bindings are **not** part of `v0.0.1`. When they are introduced, the
Python package must also derive its version dynamically rather than duplicating
it:

```toml
[project]
name = "zarr-lint"
dynamic = ["version"]
```

Maturin would then source the version from the Rust crate, keeping the single
source of truth intact.

[arrow-lint]: https://github.com/d33bs/arrow-lint
