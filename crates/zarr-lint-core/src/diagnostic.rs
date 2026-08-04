//! Diagnostic types shared by the rule engine and the reporters.
//!
//! A [`Diagnostic`] is the atomic unit of linter output: one recognized
//! problem, attached to a path, produced by a named rule at a given
//! [`Severity`]. Diagnostics are deliberately simple and serializable so that
//! both the human-readable and JSON reporters can render the same data.

use serde::Serialize;

/// How serious a finding is.
///
/// The variants are ordered from least to most severe so that the derived
/// [`Ord`] implementation can be used directly when comparing a finding
/// against a failure threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational; never fails a run on its own.
    Info,
    /// A likely problem that does not necessarily break the store.
    Warning,
    /// A definite problem with the store's structure or metadata.
    Error,
}

impl Severity {
    /// The lowercase string form used in text output and rule documentation.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single linter finding.
///
/// `path` is a store-relative display path (for example `temperature/.zarray`)
/// or, for store-level findings, the store path as supplied on the command
/// line. `detail` carries optional secondary context, such as the underlying
/// JSON parser error, and is rendered under a `Caused by:` heading in text
/// output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// Stable rule identifier, for example `metadata/invalid-json`.
    pub rule: &'static str,
    /// Default severity for the rule that produced this finding.
    pub severity: Severity,
    /// Store-relative display path the finding applies to.
    pub path: String,
    /// One-line, human-readable description of the problem.
    pub message: String,
    /// Optional secondary detail (rendered as `Caused by:` in text output).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Diagnostic {
    /// Create a diagnostic without secondary detail.
    pub fn new(
        rule: &'static str,
        severity: Severity,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule,
            severity,
            path: path.into(),
            message: message.into(),
            detail: None,
        }
    }

    /// Attach secondary detail, consuming and returning the diagnostic.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sort key giving a deterministic, file-oriented ordering.
    ///
    /// Findings are grouped by path, then by rule, then by message so that the
    /// same store always produces the same report regardless of the order in
    /// which rules or files were visited.
    pub(crate) fn sort_key(&self) -> (&str, &str, &str) {
        (&self.path, self.rule, &self.message)
    }
}
