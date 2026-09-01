//! Conservative metadata formatting for local Zarr stores.
//!
//! Formatting is deliberately limited to representation-only JSON changes:
//! deterministic object-key order, stable indentation, and one final newline.
//! It never rewrites chunks or changes parsed metadata values.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use walkdir::WalkDir;

use crate::diagnostic::Severity;
use crate::scanner::{MetadataRole, StoreOptions};

const V2_ATTRS_FILE: &str = ".zattrs";
const V2_CONSOLIDATED_FILE: &str = ".zmetadata";

/// A proposed representation-only change to one metadata document.
#[derive(Debug, Clone)]
pub struct FormatChange {
    /// Store-relative path of the metadata document, using `/` separators.
    pub rel_display: String,
    /// Filesystem path to the metadata document.
    pub path: PathBuf,
    /// Original file bytes.
    pub original_bytes: Vec<u8>,
    /// Canonical JSON bytes to write.
    pub formatted_bytes: Vec<u8>,
}

/// The complete set of formatting changes for a store.
#[derive(Debug, Clone, Default)]
pub struct FormatPlan {
    /// Metadata documents whose byte representation will change.
    pub changes: Vec<FormatChange>,
    /// Metadata documents that are already canonical.
    pub unchanged: Vec<String>,
}

impl FormatPlan {
    /// Whether the store already has canonical metadata formatting.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Debug, Clone)]
struct MetadataDocument {
    rel_display: String,
    path: PathBuf,
    bytes: Vec<u8>,
    parsed: Value,
}

impl MetadataDocument {
    fn is_store_marker(&self) -> bool {
        self.rel_display
            .rsplit_once('/')
            .map(|(_dir, name)| name)
            .unwrap_or(self.rel_display.as_str())
            != V2_ATTRS_FILE
    }
}

/// Errors that prevent safe metadata formatting.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The target is not a local filesystem store.
    #[error("fmt only supports local filesystem stores")]
    NonLocalStore,
    /// The target does not contain recognized Zarr metadata.
    #[error("no Zarr metadata found in {0}")]
    UnrecognizedStore(String),
    /// The store has lint findings that make formatting unsafe.
    #[error("store has metadata problems; run `zarr-lint check` before formatting")]
    UnsafeMetadata,
    /// A metadata file is not valid JSON.
    #[error("cannot format invalid JSON in {path}: {message}")]
    InvalidJson {
        /// Store-relative metadata path.
        path: String,
        /// JSON parser error.
        message: String,
    },
    /// Canonical JSON output did not preserve the parsed JSON value.
    #[error("proposed formatting changed parsed metadata semantics for {0}")]
    SemanticMismatch(String),
    /// The store could not be scanned.
    #[error(transparent)]
    Scan(#[from] crate::scanner::ScanError),
    /// The store could not be linted.
    #[error(transparent)]
    Lint(#[from] crate::LintError),
    /// An error occurred while walking the directory tree.
    #[error("failed to traverse store: {0}")]
    Walk(#[source] walkdir::Error),
    /// An I/O error occurred.
    #[error("failed to write {path}: {source}")]
    Io {
        /// Path that could not be read or written.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

/// Build a complete formatting plan without writing files.
pub fn plan_format_store(target: &str) -> Result<FormatPlan, FormatError> {
    if looks_like_url(target) {
        return Err(FormatError::NonLocalStore);
    }

    let report = crate::lint_store_with(target, &StoreOptions::default())?;
    let root = Path::new(target);
    let documents = discover_metadata(root)?;
    if documents.is_empty() || !documents.iter().any(MetadataDocument::is_store_marker) {
        return Err(FormatError::UnrecognizedStore(target.to_string()));
    }
    let only_unrecognized = report
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.rule == crate::rule::STRUCTURE_UNRECOGNIZED_STORE);
    if !only_unrecognized
        && report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return Err(FormatError::UnsafeMetadata);
    }

    let mut plan = FormatPlan::default();
    for document in documents {
        let formatted = canonical_json(&document.parsed)?;
        let reparsed: Value =
            serde_json::from_slice(&formatted).map_err(|err| FormatError::InvalidJson {
                path: document.rel_display.clone(),
                message: err.to_string(),
            })?;
        if reparsed != document.parsed {
            return Err(FormatError::SemanticMismatch(document.rel_display));
        }
        if formatted == document.bytes {
            plan.unchanged.push(document.rel_display);
        } else {
            plan.changes.push(FormatChange {
                rel_display: document.rel_display,
                path: document.path,
                original_bytes: document.bytes,
                formatted_bytes: formatted,
            });
        }
    }

    Ok(plan)
}

/// Return whether a local store needs formatting.
pub fn format_store_check(target: &str) -> Result<bool, FormatError> {
    Ok(!plan_format_store(target)?.is_empty())
}

/// Format a local store after planning all changes.
pub fn format_store(target: &str) -> Result<FormatPlan, FormatError> {
    let plan = plan_format_store(target)?;
    for change in &plan.changes {
        atomic_replace(&change.path, &change.formatted_bytes)?;
        let written = fs::read(&change.path).map_err(|source| FormatError::Io {
            path: change.path.display().to_string(),
            source,
        })?;
        let parsed: Value =
            serde_json::from_slice(&written).map_err(|err| FormatError::InvalidJson {
                path: change.rel_display.clone(),
                message: err.to_string(),
            })?;
        let expected: Value = serde_json::from_slice(&change.formatted_bytes).map_err(|err| {
            FormatError::InvalidJson {
                path: change.rel_display.clone(),
                message: err.to_string(),
            }
        })?;
        if parsed != expected {
            return Err(FormatError::SemanticMismatch(change.rel_display.clone()));
        }
    }

    let after = plan_format_store(target)?;
    if !after.is_empty() {
        return Err(FormatError::SemanticMismatch(target.to_string()));
    }
    Ok(plan)
}

fn discover_metadata(root: &Path) -> Result<Vec<MetadataDocument>, FormatError> {
    let mut documents = Vec::new();
    for entry in WalkDir::new(root).sort_by_file_name() {
        let entry = entry.map_err(FormatError::Walk)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str() else {
            continue;
        };
        if !is_metadata_file(name) {
            continue;
        }

        let path = entry.path();
        let bytes = fs::read(path).map_err(|source| FormatError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let rel_display = relative_display(root, path);
        let parsed: Value =
            serde_json::from_slice(&bytes).map_err(|err| FormatError::InvalidJson {
                path: rel_display.clone(),
                message: err.to_string(),
            })?;
        documents.push(MetadataDocument {
            rel_display,
            path: path.to_path_buf(),
            bytes,
            parsed,
        });
    }
    Ok(documents)
}

fn is_metadata_file(name: &str) -> bool {
    MetadataRole::from_file_name(name).is_some()
        || matches!(name, V2_ATTRS_FILE | V2_CONSOLIDATED_FILE)
}

fn canonical_json(value: &Value) -> Result<Vec<u8>, FormatError> {
    let sorted = sort_objects(value);
    let mut bytes = serde_json::to_vec_pretty(&sorted).map_err(|source| FormatError::Io {
        path: "<canonical-json>".to_string(),
        source: std::io::Error::other(source),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn sort_objects(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries: Vec<_> = object.iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let mut sorted = Map::new();
            for (key, value) in entries {
                sorted.insert(key.clone(), sort_objects(value));
            }
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.iter().map(sort_objects).collect()),
        other => other.clone(),
    }
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), FormatError> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("metadata"));
    let mut temp_name = OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(".zarr-lint-fmt.{}.tmp", std::process::id()));
    let temp_path = directory.join(temp_name);

    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|source| FormatError::Io {
            path: temp_path.display().to_string(),
            source,
        })?;
    file.write_all(bytes).map_err(|source| FormatError::Io {
        path: temp_path.display().to_string(),
        source,
    })?;
    file.sync_all().map_err(|source| FormatError::Io {
        path: temp_path.display().to_string(),
        source,
    })?;
    drop(file);

    if let Some(permissions) = permissions {
        fs::set_permissions(&temp_path, permissions).map_err(|source| FormatError::Io {
            path: temp_path.display().to_string(),
            source,
        })?;
    }
    fs::rename(&temp_path, path).map_err(|source| FormatError::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(())
}

fn relative_display(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

fn looks_like_url(target: &str) -> bool {
    let Some((scheme, rest)) = target.split_once("://") else {
        return false;
    };
    !scheme.is_empty() && !rest.is_empty() && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn plan_reports_noncanonical_metadata_only() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", "{\n  \"zarr_format\": 2\n}\n");
        write(tmp.path(), ".zattrs", r#"{"b":2,"a":[{"d":4,"c":3}]}"#);
        write(tmp.path(), "note.json", r#"{"z":0}"#);

        let plan = plan_format_store(tmp.path().to_str().unwrap()).unwrap();
        let changed: Vec<&str> = plan
            .changes
            .iter()
            .map(|change| change.rel_display.as_str())
            .collect();
        assert_eq!(changed, vec![".zattrs"]);
    }

    #[test]
    fn write_is_idempotent_and_keeps_array_order() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", "{\n  \"zarr_format\": 2\n}\n");
        write(tmp.path(), ".zattrs", r#"{"b":[3,2,1],"a":{"d":4,"c":3}}"#);

        let plan = format_store(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(plan.changes.len(), 1);
        let text = fs::read_to_string(tmp.path().join(".zattrs")).unwrap();
        assert_eq!(text, "{\n  \"a\": {\n    \"c\": 3,\n    \"d\": 4\n  },\n  \"b\": [\n    3,\n    2,\n    1\n  ]\n}\n");

        let second = plan_format_store(tmp.path().to_str().unwrap()).unwrap();
        assert!(second.is_empty());
    }

    #[test]
    fn invalid_metadata_is_refused() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", r#"{"zarr_format":2}"#);
        write(tmp.path(), ".zattrs", "{bad");

        let err = plan_format_store(tmp.path().to_str().unwrap()).unwrap_err();
        assert!(matches!(err, FormatError::InvalidJson { path, .. } if path == ".zattrs"));
    }

    #[test]
    fn unsafe_zarr_metadata_is_refused() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", r#"{"zarr_format":2}"#);
        write(
            tmp.path(),
            "a/.zarray",
            r#"{"zarr_format":2,"shape":[2,2],"chunks":[2]}"#,
        );

        let err = plan_format_store(tmp.path().to_str().unwrap()).unwrap_err();
        assert!(matches!(err, FormatError::UnsafeMetadata));
    }

    #[test]
    fn remote_targets_are_refused() {
        let err = plan_format_store("https://example.com/a.zarr").unwrap_err();
        assert!(matches!(err, FormatError::NonLocalStore));
    }

    #[test]
    fn metadata_file_names_are_exact() {
        assert!(is_metadata_file(crate::scanner::V2_GROUP_FILE));
        assert!(is_metadata_file(crate::scanner::V2_ARRAY_FILE));
        assert!(is_metadata_file(crate::scanner::V3_NODE_FILE));
        assert!(is_metadata_file(V2_ATTRS_FILE));
        assert!(is_metadata_file(V2_CONSOLIDATED_FILE));
        assert!(!is_metadata_file("metadata.json"));
        assert!(!is_metadata_file("0.0"));
    }

    #[test]
    fn consolidated_metadata_can_define_the_store_for_fmt() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            ".zmetadata",
            r#"{"metadata":{".zgroup":{"zarr_format":2}},"zarr_consolidated_format":1}"#,
        );

        let plan = plan_format_store(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].rel_display, ".zmetadata");
    }

    #[test]
    fn attrs_alone_do_not_define_a_store_for_fmt() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zattrs", r#"{"a":1}"#);

        let err = plan_format_store(tmp.path().to_str().unwrap()).unwrap_err();
        assert!(matches!(err, FormatError::UnrecognizedStore(_)));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_not_followed() {
        let store = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        write(store.path(), ".zgroup", "{\n  \"zarr_format\": 2\n}\n");
        write(outside.path(), ".zattrs", r#"{"b":2,"a":1}"#);
        std::os::unix::fs::symlink(outside.path(), store.path().join("linked")).unwrap();

        let plan = format_store(store.path().to_str().unwrap()).unwrap();
        assert!(plan.is_empty());
        assert_eq!(
            fs::read_to_string(outside.path().join(".zattrs")).unwrap(),
            r#"{"b":2,"a":1}"#
        );
    }
}
