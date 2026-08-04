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

pub mod diagnostic;
pub mod model;
pub mod rule;
pub mod scanner;

use std::path::Path;

use serde::Serialize;

pub use diagnostic::{Diagnostic, Severity};
pub use model::{LoadedStore, NodeKind, ParsedMetadata, ZarrNode, ZarrVersion};
pub use rule::{RuleInfo, RULES};
pub use scanner::{ScanError, StoreScan};

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

/// Scan, parse, and lint the store rooted at `root`.
///
/// Returns a [`Report`] whose diagnostics are sorted deterministically. Returns
/// [`LintError`] only for store-access failures; a store that is simply not a
/// Zarr store yields a report containing a `structure/unrecognized-store`
/// finding rather than an error.
pub fn lint_store(root: &Path) -> Result<Report, LintError> {
    let store_display = root.display().to_string();
    let scan = scanner::scan_store(root)?;
    let loaded = model::load(&scan);

    let mut diagnostics = rule::evaluate(&scan, &loaded, &store_display);
    diagnostics.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    Ok(Report {
        version: VERSION,
        store: store_display,
        diagnostics,
    })
}

/// Scan and parse the store without running any rules.
///
/// This supports inspection tooling that wants to enumerate the normalized
/// nodes (see [`ZarrNode`]) discovered in a store.
pub fn load_store(root: &Path) -> Result<(StoreScan, LoadedStore), LintError> {
    let scan = scanner::scan_store(root)?;
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

        let report = lint_store(tmp.path()).unwrap();
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
        let err = lint_store(Path::new("/no/such/store/zzz")).unwrap_err();
        assert!(matches!(err, LintError::Scan(ScanError::NotFound(_))));
    }
}
