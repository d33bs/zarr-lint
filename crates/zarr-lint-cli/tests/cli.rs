//! End-to-end tests that drive the built `zarr-lint` binary against the
//! vendored fixtures. These exercise the full pipeline (discovery, parsing,
//! rules, reporting, exit codes) exactly as a user would.

use std::fs;
use std::path::{Path, PathBuf};
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

fn write(dir: &Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
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

// ---- fmt ---------------------------------------------------------------

#[test]
fn fmt_dry_run_reports_changes_without_writing() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(tmp.path(), ".zgroup", "{\n  \"zarr_format\": 2\n}\n");
    write(tmp.path(), ".zattrs", r#"{"b":2,"a":1}"#);

    let out = run(&["fmt", tmp.path().to_str().unwrap()]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("Would format 1 metadata file"));
    assert_eq!(
        fs::read_to_string(tmp.path().join(".zattrs")).unwrap(),
        r#"{"b":2,"a":1}"#
    );
}

#[test]
fn fmt_check_fails_when_formatting_is_needed() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(tmp.path(), ".zgroup", "{\n  \"zarr_format\": 2\n}\n");
    write(tmp.path(), ".zattrs", r#"{"b":2,"a":1}"#);

    let out = run(&["fmt", tmp.path().to_str().unwrap(), "--check"]);
    assert_eq!(code(&out), 1, "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("1 metadata file"));
}

#[test]
fn fmt_json_output_reports_changes() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(tmp.path(), ".zgroup", "{\n  \"zarr_format\": 2\n}\n");
    write(tmp.path(), ".zattrs", r#"{"b":2,"a":1}"#);

    let out = run(&[
        "fmt",
        tmp.path().to_str().unwrap(),
        "--format",
        "json",
        "--check",
    ]);
    assert_eq!(code(&out), 1, "stderr: {}", stderr(&out));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["mode"], "check");
    assert_eq!(json["would_change"], true);
    assert_eq!(json["changed_count"], 1);
    assert_eq!(json["changes"].as_array().unwrap()[0], ".zattrs");
}

#[test]
fn fmt_write_formats_metadata_and_leaves_unrelated_json() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(tmp.path(), ".zgroup", "{\n  \"zarr_format\": 2\n}\n");
    write(tmp.path(), ".zattrs", r#"{"b":[2,1],"a":{"d":4,"c":3}}"#);
    write(tmp.path(), "app.json", r#"{"z":0,"a":1}"#);

    let out = run(&["fmt", tmp.path().to_str().unwrap(), "--write"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("Formatted 1 metadata file"));
    assert_eq!(
        fs::read_to_string(tmp.path().join(".zattrs")).unwrap(),
        "{\n  \"a\": {\n    \"c\": 3,\n    \"d\": 4\n  },\n  \"b\": [\n    2,\n    1\n  ]\n}\n"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("app.json")).unwrap(),
        r#"{"z":0,"a":1}"#
    );

    let check = run(&["fmt", tmp.path().to_str().unwrap(), "--check"]);
    assert_eq!(code(&check), 0, "stderr: {}", stderr(&check));
}

#[test]
fn fmt_formats_consolidated_metadata_from_cli() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(
        tmp.path(),
        ".zmetadata",
        r#"{"metadata":{".zgroup":{"zarr_format":2}},"zarr_consolidated_format":1}"#,
    );

    let out = run(&["fmt", tmp.path().to_str().unwrap(), "--write"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("Formatted 1 metadata file"));
    assert_eq!(
        fs::read_to_string(tmp.path().join(".zmetadata")).unwrap(),
        "{\n  \"metadata\": {\n    \".zgroup\": {\n      \"zarr_format\": 2\n    }\n  },\n  \"zarr_consolidated_format\": 1\n}\n"
    );
}

#[test]
fn fmt_refuses_invalid_metadata() {
    let tmp = tempfile::TempDir::new().unwrap();
    write(tmp.path(), ".zgroup", r#"{"zarr_format":2}"#);
    write(tmp.path(), ".zattrs", "{bad");

    let out = run(&["fmt", tmp.path().to_str().unwrap()]);
    assert_eq!(code(&out), 3);
    assert!(stderr(&out).contains("cannot format invalid JSON"));
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
