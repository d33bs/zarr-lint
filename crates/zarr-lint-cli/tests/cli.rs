//! End-to-end tests that drive the built `zarr-lint` binary against the
//! vendored fixtures. These exercise the full pipeline (discovery, parsing,
//! rules, reporting, exit codes) exactly as a user would.

use std::path::PathBuf;
use std::process::{Command, Output};

/// Run the built binary with `args` and return its output.
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zarr-lint"))
        .args(args)
        .output()
        .expect("failed to execute zarr-lint binary")
}

/// The exit status code, or a sentinel if the process was signalled.
fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Absolute path to a fixture under `test-data/`.
fn fixture(rel: &str) -> String {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    repo_root
        .join("test-data")
        .join(rel)
        .to_string_lossy()
        .into_owned()
}

const UPSTREAM: &str = "upstream/duckdb-zarr";

// ---- versioning ---------------------------------------------------------

#[test]
fn version_flag_reports_workspace_version() {
    let out = run(&["--version"]);
    assert_eq!(code(&out), 0);
    assert_eq!(
        stdout(&out),
        format!("zarr-lint {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn cli_and_core_versions_agree() {
    // Both crates inherit the workspace version, so the CLI's compile-time
    // version must match the core crate's exported VERSION constant.
    assert_eq!(env!("CARGO_PKG_VERSION"), zarr_lint_core::VERSION);
}

#[test]
fn version_subcommand_verbose_includes_profile() {
    let out = run(&["version", "--verbose"]);
    assert_eq!(code(&out), 0);
    let text = stdout(&out);
    assert!(text.starts_with(&format!("zarr-lint {}", env!("CARGO_PKG_VERSION"))));
    assert!(text.contains("commit:"));
    assert!(text.contains("build profile:"));
}

// ---- valid fixtures -----------------------------------------------------

#[test]
fn valid_v2_store_passes() {
    let out = run(&["check", &fixture(&format!("{UPSTREAM}/simple_v2.zarr"))]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("No problems found"));
}

#[test]
fn valid_v3_store_passes() {
    let out = run(&["check", &fixture(&format!("{UPSTREAM}/simple_v3.zarr"))]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
}

#[test]
fn shorthand_form_checks_store() {
    // No `check` subcommand: `zarr-lint <PATH>`.
    let out = run(&[&fixture(&format!("{UPSTREAM}/simple_v2.zarr"))]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("No problems found"));
}

#[test]
fn valid_json_output_has_empty_diagnostics_and_version() {
    let out = run(&[
        "check",
        "--format",
        "json",
        &fixture(&format!("{UPSTREAM}/simple_v2.zarr")),
    ]);
    assert_eq!(code(&out), 0);
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["diagnostics"].as_array().unwrap().len(), 0);
}

// ---- one positive case per rule ----------------------------------------

/// Each invalid fixture should fail with exit code 1 and surface exactly the
/// rule it was constructed to trigger.
#[test]
fn each_invalid_fixture_triggers_its_rule() {
    let cases = [
        ("invalid/invalid-json.zarr", "metadata/invalid-json"),
        (
            "invalid/missing-zarr-format.zarr",
            "metadata/missing-required-field",
        ),
        (
            "invalid/unsupported-version.zarr",
            "metadata/unsupported-format-version",
        ),
        ("invalid/rank-mismatch.zarr", "array/rank-mismatch"),
        (
            "invalid/conflicting-node-type.zarr",
            "structure/conflicting-node-type",
        ),
        ("invalid/not-a-zarr", "structure/unrecognized-store"),
    ];

    for (rel, rule) in cases {
        let out = run(&["check", "--format", "json", &fixture(rel)]);
        assert_eq!(
            code(&out),
            1,
            "{rel} should exit 1; stderr: {}",
            stderr(&out)
        );
        let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
        let diagnostics = json["diagnostics"].as_array().unwrap();
        assert_eq!(
            diagnostics.len(),
            1,
            "{rel} should report exactly one finding"
        );
        assert_eq!(diagnostics[0]["rule"], rule, "wrong rule for {rel}");
    }
}

// ---- exit codes and thresholds -----------------------------------------

#[test]
fn fail_on_never_exits_zero_despite_findings() {
    let out = run(&[
        "check",
        "--fail-on",
        "never",
        &fixture("invalid/rank-mismatch.zarr"),
    ]);
    assert_eq!(code(&out), 0);
    // The finding is still reported, just not fatal.
    assert!(stdout(&out).contains("array/rank-mismatch"));
}

#[test]
fn missing_store_path_is_access_failure() {
    let out = run(&["check", &fixture("does/not/exist")]);
    assert_eq!(code(&out), 3);
    assert!(stderr(&out).contains("does not exist"));
}

#[test]
fn no_path_is_usage_error() {
    let out = run(&["check"]);
    assert_eq!(code(&out), 2);
}

#[test]
fn invalid_flag_is_usage_error() {
    let out = run(&["check", "--format", "xml", &fixture("invalid/not-a-zarr")]);
    assert_eq!(code(&out), 2);
}

// ---- inspect ------------------------------------------------------------

#[test]
fn inspect_lists_nodes_from_both_versions() {
    let v2 = run(&["inspect", &fixture(&format!("{UPSTREAM}/simple_v2.zarr"))]);
    assert_eq!(code(&v2), 0);
    let v2_text = stdout(&v2);
    assert!(v2_text.contains("v2 group"));
    assert!(v2_text.contains("temperature"));

    let v3 = run(&["inspect", &fixture(&format!("{UPSTREAM}/simple_v3.zarr"))]);
    assert_eq!(code(&v3), 0);
    assert!(stdout(&v3).contains("v3 array"));
}

// ---- ecosystem: no false positives -------------------------------------

/// Real stores written by popular ecosystem tools must lint cleanly. This is
/// the "no false positives" guard: if a mainstream tool's output trips a rule,
/// that is a zarr-lint bug, not a bad store.
///
/// Fixtures are generated by `tools/generate_fixtures.py`.
#[test]
fn real_ecosystem_stores_have_no_findings() {
    let stores = [
        "generated/zarr-python/v2.zarr",
        "generated/zarr-python/v3.zarr",
        "generated/xarray/dataset_v2.zarr",
        "generated/xarray/dataset_v3.zarr",
        "generated/tensorstore/array_v2.zarr",
        "generated/tensorstore/array_v3.zarr",
    ];
    for store in stores {
        let out = run(&["check", "--format", "json", &fixture(store)]);
        assert_eq!(
            code(&out),
            0,
            "{store} should be clean; stderr: {}",
            stderr(&out)
        );
        let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
        let diagnostics = json["diagnostics"].as_array().unwrap();
        assert!(
            diagnostics.is_empty(),
            "{store} produced unexpected findings: {diagnostics:?}"
        );
    }
}
