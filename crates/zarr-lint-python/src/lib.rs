//! Python bindings for zarr-lint.
//!
//! The module deliberately stays thin: the command line entry point delegates to
//! the exact same [`zarr_lint_cli::run`] used by the native binary, and the
//! structured helpers return JSON strings that the Python layer parses. This
//! keeps behavior identical across the Rust and Python front ends.

use std::path::PathBuf;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use zarr_lint_core::{lint_store, RULES, VERSION};

/// Run the full `zarr-lint` command line with `args` (including the program
/// name as the first element) and return the process exit code.
#[pyfunction]
fn run_cli(args: Vec<String>) -> i32 {
    zarr_lint_cli::run(args)
}

/// Lint the store at `path` and return the report as a JSON string.
#[pyfunction]
fn lint_store_json(path: String) -> PyResult<String> {
    let report = lint_store(&PathBuf::from(path)).map_err(to_py_error)?;
    serde_json::to_string_pretty(&report).map_err(to_py_error)
}

/// Return the built-in rule registry as a JSON string.
#[pyfunction]
fn rules_json() -> PyResult<String> {
    let rules: Vec<serde_json::Value> = RULES
        .iter()
        .map(|rule| {
            serde_json::json!({
                "id": rule.id,
                "default_severity": rule.default_severity.as_str(),
                "summary": rule.summary,
            })
        })
        .collect();
    serde_json::to_string_pretty(&rules).map_err(to_py_error)
}

/// The zarr-lint version (the single authoritative Cargo workspace version).
#[pyfunction]
fn version() -> &'static str {
    VERSION
}

fn to_py_error(error: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(error.to_string())
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", VERSION)?;
    module.add_function(wrap_pyfunction!(run_cli, module)?)?;
    module.add_function(wrap_pyfunction!(lint_store_json, module)?)?;
    module.add_function(wrap_pyfunction!(rules_json, module)?)?;
    module.add_function(wrap_pyfunction!(version, module)?)?;
    Ok(())
}
