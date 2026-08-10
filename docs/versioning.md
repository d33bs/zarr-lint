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

The committed workspace version is a **development placeholder**; it is not
expected to track the latest release. At release time the tag is the source of
truth: [`tools/sync_release_version.py`](../tools/sync_release_version.py)
rewrites the workspace version to match the tag inside each release build
checkout (an ephemeral edit, never committed), so the published artifacts carry
the tag's version. This mirrors the dynamic release versioning used by the
sibling [`arrow-lint`][arrow-lint] project. Because the tag drives the version,
publishing a draft release no longer requires a manual `Cargo.toml` bump
beforehand.

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

[`tools/check_versions.py`](../tools/check_versions.py) enforces the invariant
during development. It is run in CI on every push.

```console
$ python tools/check_versions.py
OK: version 0.0.1 is consistent across 3 crates, pyproject dynamic version, CLI.

$ python tools/check_versions.py --tag v0.0.1
OK: version 0.0.1 is consistent across 3 crates, pyproject dynamic version, CLI, tag v0.0.1.
```

Its `--tag` mode checks that the committed version already equals a tag — useful
for a manual pre-flight check, but it is **not** the release gate. The release
workflows use [`tools/sync_release_version.py`](../tools/sync_release_version.py)
to set the version from the tag at build time (see
[`.github/workflows/release.yml`](../.github/workflows/release.yml) and
[`.github/workflows/publish-pypi.yml`](../.github/workflows/publish-pypi.yml)).

It checks that:

1. Every workspace crate resolves to the same version.
2. The crates expected to be published are present.
3. The CLI reports that version.
4. `pyproject.toml` declares the version as dynamic (never static).
5. A supplied release tag equals `v<version>`.

## Python packaging

The Python package derives its version dynamically from the same Cargo source;
it never pins its own:

```toml
[project]
name = "zarr-lint"
dynamic = ["version"]
```

Maturin sources the version from the `zarr-lint-python` crate (which inherits
the workspace version), so `pip install zarr-lint` and `zarr-lint --version`
report the same value. The build is proven end to end:

```console
$ maturin build --release
📦 Built wheel ... zarr_lint-0.0.1-cp311-abi3-<platform>.whl

$ python -c "import zarr_lint; print(zarr_lint.__version__)"
0.0.1
```

The release workflow publishes wheels and an sdist to PyPI on a GitHub release.
The release tag drives the version: `tools/sync_release_version.py` rewrites the
workspace version to match the tag in each build checkout before maturin
builds, so the published wheel version follows the tag (see
[`.github/workflows/publish-pypi.yml`](../.github/workflows/publish-pypi.yml)).

[arrow-lint]: https://github.com/d33bs/arrow-lint
