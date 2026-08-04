//! Thin binary wrapper around the `zarr-lint` CLI library.
//!
//! All logic lives in the library crate so the native binary and the Python
//! bindings share one implementation; see [`zarr_lint_cli::run`].

use std::process::ExitCode;

fn main() -> ExitCode {
    // Exit codes are in the range 0..=3, so the cast to u8 is always exact.
    ExitCode::from(zarr_lint_cli::run(std::env::args_os()) as u8)
}
