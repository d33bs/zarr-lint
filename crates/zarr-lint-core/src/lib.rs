//! Core store discovery, metadata model, and rule engine for `zarr-lint`.
//!
//! The public entry point is [`lint_store`], which runs the whole `v0.0.1`
//! pipeline:
//!
//! ```text
//! store path -> discovery -> metadata loading -> normalized model
//!            -> rule set -> sorted diagnostics -> Report
//! ```
//!
//! The report is serializable and carries enough information for both the
//! human-readable and JSON reporters in the CLI. Nothing in this crate reads
//! chunk data or claims full Zarr specification conformance; see the project
//! documentation for the (deliberately small) scope of the initial rule set.

mod cloud;
pub mod diagnostic;
pub mod model;
mod remote;
pub mod rule;
pub mod scanner;

use serde::Serialize;

pub use diagnostic::{Diagnostic, Severity};
pub use model::{LoadedStore, NodeKind, ParsedMetadata, ZarrNode, ZarrVersion};
pub use rule::{RuleInfo, RULES};
pub use scanner::{ScanError, StoreOptions, StoreScan};

/// The single authoritative project version, derived from the Cargo workspace
/// package version at compile time. Never hard-code a version elsewhere.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A complete lint report for a single store.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// The project version that produced the report.
    pub version: &'static str,
    /// The store path as supplied to [`lint_store`].
    pub store: String,
    /// All findings, in a deterministic order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Report {
    /// Count findings by severity, returned as `(errors, warnings, infos)`.
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut errors = 0;
        let mut warnings = 0;
        let mut infos = 0;
        for diagnostic in &self.diagnostics {
            match diagnostic.severity {
                Severity::Error => errors += 1,
                Severity::Warning => warnings += 1,
                Severity::Info => infos += 1,
            }
        }
        (errors, warnings, infos)
    }

    /// Whether any finding is at least as severe as `threshold`.
    pub fn has_at_or_above(&self, threshold: Severity) -> bool {
        self.diagnostics.iter().any(|d| d.severity >= threshold)
    }
}

/// Errors that prevent producing a report at all.
///
/// These are store-access or internal execution failures and map to CLI exit
/// code `3`, as distinct from lint findings (exit code `1`).
#[derive(Debug, thiserror::Error)]
pub enum LintError {
    /// The store could not be scanned.
    #[error(transparent)]
    Scan(#[from] ScanError),
}

/// Scan, parse, and lint the store at `target`.
///
/// `target` is a local filesystem path or an `http(s)://` URL. Returns a
/// [`Report`] whose diagnostics are sorted deterministically. Returns
/// [`LintError`] only for store-access failures; a location that is simply not a
/// Zarr store yields a report containing a `structure/unrecognized-store`
/// finding rather than an error.
pub fn lint_store(target: &str) -> Result<Report, LintError> {
    lint_store_with(target, &StoreOptions::default())
}

/// Like [`lint_store`], with explicit [`StoreOptions`] (for example anonymous
/// cloud access).
pub fn lint_store_with(target: &str, options: &StoreOptions) -> Result<Report, LintError> {
    let scan = scanner::scan_store(target, options)?;
    let loaded = model::load(&scan);

    let mut diagnostics = rule::evaluate(&scan, &loaded, target);
    diagnostics.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    Ok(Report {
        version: VERSION,
        store: target.to_string(),
        diagnostics,
    })
}

/// Scan and parse the store at `target` (a path or URL) without running rules.
///
/// This supports inspection tooling that wants to enumerate the normalized
/// nodes (see [`ZarrNode`]) discovered in a store.
pub fn load_store(
    target: &str,
    options: &StoreOptions,
) -> Result<(StoreScan, LoadedStore), LintError> {
    let scan = scanner::scan_store(target, options)?;
    let loaded = model::load(&scan);
    Ok((scan, loaded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn version_is_populated_from_cargo() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn report_is_sorted_and_counts_match() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".zgroup"), r#"{"zarr_format":2}"#).unwrap();
        // Two arrays with distinct problems, created out of sorted order.
        fs::create_dir_all(tmp.path().join("z_arr")).unwrap();
        fs::write(
            tmp.path().join("z_arr/.zarray"),
            r#"{"zarr_format":2,"shape":[4,3],"chunks":[2],"dtype":"<f8"}"#,
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("a_arr")).unwrap();
        fs::write(tmp.path().join("a_arr/.zarray"), "{bad").unwrap();

        let report = lint_store(tmp.path().to_str().unwrap()).unwrap();
        // Deterministic: sorted by path, so a_arr precedes z_arr.
        let paths: Vec<&str> = report.diagnostics.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, vec!["a_arr/.zarray", "z_arr/.zarray"]);
        assert_eq!(report.counts(), (2, 0, 0));
        assert!(report.has_at_or_above(Severity::Error));
        // Errors also clear the lower warning threshold.
        assert!(report.has_at_or_above(Severity::Warning));
    }

    #[test]
    fn missing_store_is_a_lint_error() {
        let err = lint_store("/no/such/store/zzz").unwrap_err();
        assert!(matches!(err, LintError::Scan(ScanError::NotFound(_))));
    }

    #[test]
    fn remote_store_reads_over_http() {
        // A local HTTP server standing in for a remote store with consolidated
        // metadata; proves `lint_store` accepts and reads http(s):// targets.
        let consolidated = r#"{"zarr_consolidated_format":1,"metadata":{
            ".zgroup":{"zarr_format":2},
            "t/.zarray":{"zarr_format":2,"shape":[4,3],"chunks":[2]}
        }}"#;
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let body = consolidated.to_string();
        let handle = std::thread::spawn(move || loop {
            let request = server.recv().unwrap();
            let is_meta = request.url().ends_with("/.zmetadata");
            let response = if is_meta {
                tiny_http::Response::from_string(body.clone()).with_status_code(200)
            } else {
                tiny_http::Response::from_string(String::new()).with_status_code(404)
            };
            request.respond(response).unwrap();
            if is_meta {
                break;
            }
        });

        let report = lint_store(&format!("http://127.0.0.1:{port}/s.zarr")).unwrap();
        handle.join().unwrap();

        // The consolidated array `t` has a shape/chunk rank mismatch.
        let rules: Vec<&str> = report.diagnostics.iter().map(|d| d.rule).collect();
        assert_eq!(rules, vec!["array/rank-mismatch"]);
    }
}
