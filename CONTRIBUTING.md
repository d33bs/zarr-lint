# Contributing to zarr-lint

Thanks for your interest in improving zarr-lint! Contributions of all kinds are
welcome — bug reports, fixes, new rules, docs, and fixtures.

By participating you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Getting started

zarr-lint is a Rust workspace with optional Python bindings. You need
[Rust](https://rustup.rs/) 1.82+; for the Python package you also need
[uv](https://docs.astral.sh/uv/).

```bash
git clone https://github.com/d33bs/zarr-lint
cd zarr-lint
cargo build
cargo test
```

## Development checks

These mirror CI; please run them before opening a pull request.

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
python tools/check_versions.py
```

We use [prek](https://github.com/j178/prek) (a fast pre-commit runner) for
repository-wide checks (formatting, spelling, workflow and security linting):

```bash
prek run --all-files      # or: pre-commit run --all-files
```

### Python bindings

```bash
uv venv
uv pip install 'maturin>=1.9,<2' pytest
uv run maturin develop --uv
uv run pytest
```

## Adding a rule

Each rule lives in [`crates/zarr-lint-core/src/rule.rs`](crates/zarr-lint-core/src/rule.rs)
and needs a stable identifier, a documented default severity, and **both** a
fixture that triggers it and one that does not. See
[docs/rules.md](docs/rules.md) and [docs/test-corpus.md](docs/test-corpus.md).

## Versioning

The project version has a single source of truth: the Cargo workspace version.
Do not hard-code versions elsewhere. See [docs/versioning.md](docs/versioning.md).

## Pull requests

Open a pull request against `main`. CI runs formatting, Clippy, tests, version
consistency, workflow security (zizmor), and the Python build. Please fill out
the pull request template and ensure all checks pass.
