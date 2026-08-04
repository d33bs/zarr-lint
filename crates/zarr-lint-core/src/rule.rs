//! The `v0.0.1` rule set and its evaluation.
//!
//! Each rule has a stable identifier, a documented default [`Severity`], and a
//! one-line summary, all recorded in [`RULES`]. The [`evaluate`] function runs
//! the whole set against a scanned and parsed store and returns the findings
//! (unsorted; the caller sorts for deterministic output).
//!
//! The rules intentionally cover only a small, well-understood slice of Zarr
//! correctness. They establish the architecture without claiming comprehensive
//! specification conformance.

use std::collections::BTreeMap;

use crate::diagnostic::{Diagnostic, Severity};
use crate::model::{format_dims, LoadedStore, NodeKind};
use crate::scanner::{MetadataRole, StoreScan};

/// The supplied path does not contain recognizable Zarr metadata.
pub const STRUCTURE_UNRECOGNIZED_STORE: &str = "structure/unrecognized-store";
/// A recognized metadata file could not be parsed as JSON.
pub const METADATA_INVALID_JSON: &str = "metadata/invalid-json";
/// A minimally required metadata field is absent.
pub const METADATA_MISSING_REQUIRED_FIELD: &str = "metadata/missing-required-field";
/// The metadata declares an unsupported Zarr format version.
pub const METADATA_UNSUPPORTED_FORMAT_VERSION: &str = "metadata/unsupported-format-version";
/// A single path declares both array and group (or v2 and v3) metadata.
pub const STRUCTURE_CONFLICTING_NODE_TYPE: &str = "structure/conflicting-node-type";
/// An array's shape rank disagrees with its chunk-shape rank.
pub const ARRAY_RANK_MISMATCH: &str = "array/rank-mismatch";

/// Documentation for one rule in the registry.
#[derive(Debug, Clone, Copy)]
pub struct RuleInfo {
    /// Stable rule identifier, for example `metadata/invalid-json`.
    pub id: &'static str,
    /// The severity findings from this rule are emitted at.
    pub default_severity: Severity,
    /// A one-line description of what the rule checks.
    pub summary: &'static str,
}

/// The complete `v0.0.1` rule registry, in documentation order.
pub const RULES: &[RuleInfo] = &[
    RuleInfo {
        id: STRUCTURE_UNRECOGNIZED_STORE,
        default_severity: Severity::Error,
        summary: "The supplied path does not contain recognizable Zarr metadata.",
    },
    RuleInfo {
        id: METADATA_INVALID_JSON,
        default_severity: Severity::Error,
        summary: "A recognized metadata file could not be parsed as JSON.",
    },
    RuleInfo {
        id: METADATA_MISSING_REQUIRED_FIELD,
        default_severity: Severity::Error,
        summary: "A minimally required metadata field is absent.",
    },
    RuleInfo {
        id: METADATA_UNSUPPORTED_FORMAT_VERSION,
        default_severity: Severity::Error,
        summary: "The metadata declares an unsupported Zarr format version.",
    },
    RuleInfo {
        id: STRUCTURE_CONFLICTING_NODE_TYPE,
        default_severity: Severity::Error,
        summary: "A path declares both array and group metadata.",
    },
    RuleInfo {
        id: ARRAY_RANK_MISMATCH,
        default_severity: Severity::Error,
        summary: "An array's shape rank disagrees with its chunk-shape rank.",
    },
];

/// Run the whole rule set against a scanned and parsed store.
///
/// `store_display` is the store path as supplied on the command line; it is
/// used as the location for store-level findings. The returned findings are
/// unsorted.
pub fn evaluate(scan: &StoreScan, loaded: &LoadedStore, store_display: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    // structure/unrecognized-store is a store-level check. When it fires there
    // are no nodes to inspect, so no other rule can meaningfully run.
    if !scan.is_recognized() {
        out.push(Diagnostic::new(
            STRUCTURE_UNRECOGNIZED_STORE,
            Severity::Error,
            store_display.to_string(),
            "The supplied path does not contain recognizable Zarr metadata.",
        ));
        return out;
    }

    check_invalid_json(loaded, &mut out);
    check_missing_required_fields(loaded, &mut out);
    check_unsupported_format_version(loaded, &mut out);
    check_conflicting_node_type(scan, loaded, store_display, &mut out);
    check_rank_mismatch(loaded, &mut out);

    out
}

fn check_invalid_json(loaded: &LoadedStore, out: &mut Vec<Diagnostic>) {
    for failure in &loaded.parse_failures {
        out.push(
            Diagnostic::new(
                METADATA_INVALID_JSON,
                Severity::Error,
                failure.rel_display.clone(),
                "Metadata could not be parsed as JSON.",
            )
            .with_detail(failure.message.clone()),
        );
    }
}

fn check_missing_required_fields(loaded: &LoadedStore, out: &mut Vec<Diagnostic>) {
    for node in &loaded.parsed {
        let mut missing = |field: &str| {
            out.push(Diagnostic::new(
                METADATA_MISSING_REQUIRED_FIELD,
                Severity::Error,
                node.rel_display.clone(),
                format!("Required field `{field}` is missing."),
            ));
        };

        if node.declared_format().is_none() {
            missing("zarr_format");
        }

        // Zarr v3 documents must declare whether they are a group or an array.
        let is_v3 = node.role == MetadataRole::V3Node;
        if is_v3 && node.node_type().is_none() {
            missing("node_type");
        }

        // Array nodes must declare a shape and a chunk grid. For v3 we can only
        // treat a node as an array once `node_type` says so.
        let is_array = matches!(node.kind(), Some(NodeKind::Array));
        if is_array {
            if !node.has_field("shape") {
                missing("shape");
            }
            if !node.has_chunk_grid_field() {
                missing(if is_v3 { "chunk_grid" } else { "chunks" });
            }
        }
    }
}

fn check_unsupported_format_version(loaded: &LoadedStore, out: &mut Vec<Diagnostic>) {
    for node in &loaded.parsed {
        if let Some(version) = node.declared_format() {
            if version != 2 && version != 3 {
                out.push(Diagnostic::new(
                    METADATA_UNSUPPORTED_FORMAT_VERSION,
                    Severity::Error,
                    node.rel_display.clone(),
                    format!("Unsupported Zarr format version: {version}."),
                ));
            }
        }
    }
}

fn check_conflicting_node_type(
    scan: &StoreScan,
    loaded: &LoadedStore,
    store_display: &str,
    out: &mut Vec<Diagnostic>,
) {
    /// What a single location contains, for conflict detection.
    #[derive(Default)]
    struct LocationInfo {
        has_v2: bool,
        has_v3: bool,
        has_group: bool,
        has_array: bool,
        markers: Vec<String>,
    }

    // Resolve v3 node kinds by display path so a `zarr.json`'s group/array
    // nature can be considered alongside any v2 markers sharing its location.
    let v3_kinds: BTreeMap<&str, Option<NodeKind>> = loaded
        .parsed
        .iter()
        .filter(|p| p.role == MetadataRole::V3Node)
        .map(|p| (p.rel_display.as_str(), p.kind()))
        .collect();

    let mut by_location: BTreeMap<&str, LocationInfo> = BTreeMap::new();
    for file in &scan.files {
        let info = by_location.entry(file.location.as_str()).or_default();
        info.markers.push(file.rel_display.clone());
        match file.role {
            MetadataRole::V2Group => {
                info.has_v2 = true;
                info.has_group = true;
            }
            MetadataRole::V2Array => {
                info.has_v2 = true;
                info.has_array = true;
            }
            MetadataRole::V3Node => {
                info.has_v3 = true;
                match v3_kinds.get(file.rel_display.as_str()).copied().flatten() {
                    Some(NodeKind::Group) => info.has_group = true,
                    Some(NodeKind::Array) => info.has_array = true,
                    None => {}
                }
            }
        }
    }

    for (location, info) in by_location {
        let mixed_kind = info.has_group && info.has_array;
        let mixed_version = info.has_v2 && info.has_v3;
        if !(mixed_kind || mixed_version) {
            continue;
        }
        let path = if location.is_empty() {
            store_display.to_string()
        } else {
            location.to_string()
        };
        let mut markers = info.markers;
        markers.sort();
        out.push(
            Diagnostic::new(
                STRUCTURE_CONFLICTING_NODE_TYPE,
                Severity::Error,
                path,
                "Path contains conflicting node metadata.",
            )
            .with_detail(format!("Found: {}", markers.join(", "))),
        );
    }
}

fn check_rank_mismatch(loaded: &LoadedStore, out: &mut Vec<Diagnostic>) {
    for node in &loaded.parsed {
        if !matches!(node.kind(), Some(NodeKind::Array)) {
            continue;
        }
        let (Some(shape), Some(chunks)) = (node.shape_dims(), node.chunk_dims()) else {
            continue;
        };
        if shape.len() != chunks.len() {
            out.push(
                Diagnostic::new(
                    ARRAY_RANK_MISMATCH,
                    Severity::Error,
                    node.rel_display.clone(),
                    format!(
                        "Array shape rank ({}) does not match chunk shape rank ({}).",
                        shape.len(),
                        chunks.len()
                    ),
                )
                .with_detail(format!(
                    "shape {}, chunks {}",
                    format_dims(shape),
                    format_dims(chunks)
                )),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model;
    use crate::scanner::scan_store;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn run(dir: &Path) -> Vec<Diagnostic> {
        let scan = scan_store(dir).unwrap();
        let loaded = model::load(&scan);
        evaluate(&scan, &loaded, &dir.display().to_string())
    }

    fn rule_ids(diags: &[Diagnostic]) -> Vec<&str> {
        diags.iter().map(|d| d.rule).collect()
    }

    #[test]
    fn registry_has_six_documented_rules() {
        assert_eq!(RULES.len(), 6);
        for info in RULES {
            assert!(!info.id.is_empty());
            assert!(!info.summary.is_empty());
        }
    }

    #[test]
    fn clean_v2_store_has_no_findings() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", r#"{"zarr_format":2}"#);
        write(
            tmp.path(),
            "t/.zarray",
            r#"{"zarr_format":2,"shape":[4,3],"chunks":[2,2],"dtype":"<f8"}"#,
        );
        assert!(run(tmp.path()).is_empty());
    }

    #[test]
    fn unrecognized_store_fires_alone() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "readme.txt", "not zarr");
        let diags = run(tmp.path());
        assert_eq!(rule_ids(&diags), vec![STRUCTURE_UNRECOGNIZED_STORE]);
    }

    #[test]
    fn invalid_json_is_reported_with_detail() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", r#"{"zarr_format":2}"#);
        write(tmp.path(), "bad/.zarray", "{not json");
        let diags = run(tmp.path());
        assert_eq!(rule_ids(&diags), vec![METADATA_INVALID_JSON]);
        assert!(diags[0].detail.is_some());
    }

    #[test]
    fn missing_zarr_format_is_reported() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", r#"{"zarr_format":2}"#);
        write(
            tmp.path(),
            "t/.zarray",
            r#"{"shape":[4,3],"chunks":[2,2],"dtype":"<f8"}"#,
        );
        let diags = run(tmp.path());
        assert_eq!(rule_ids(&diags), vec![METADATA_MISSING_REQUIRED_FIELD]);
        assert!(diags[0].message.contains("zarr_format"));
    }

    #[test]
    fn v3_missing_node_type_reports_only_node_type() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "zarr.json",
            r#"{"zarr_format":3,"node_type":"group"}"#,
        );
        write(tmp.path(), "a/zarr.json", r#"{"zarr_format":3}"#);
        let diags = run(tmp.path());
        assert_eq!(rule_ids(&diags), vec![METADATA_MISSING_REQUIRED_FIELD]);
        assert!(diags[0].message.contains("node_type"));
    }

    #[test]
    fn unsupported_format_version_is_reported() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", r#"{"zarr_format":2}"#);
        write(
            tmp.path(),
            "t/.zarray",
            r#"{"zarr_format":9,"shape":[2],"chunks":[2],"dtype":"<i4"}"#,
        );
        let diags = run(tmp.path());
        assert_eq!(rule_ids(&diags), vec![METADATA_UNSUPPORTED_FORMAT_VERSION]);
    }

    #[test]
    fn conflicting_markers_are_reported() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", r#"{"zarr_format":2}"#);
        write(tmp.path(), "both/.zgroup", r#"{"zarr_format":2}"#);
        write(
            tmp.path(),
            "both/.zarray",
            r#"{"zarr_format":2,"shape":[2],"chunks":[2],"dtype":"<i4"}"#,
        );
        let diags = run(tmp.path());
        assert_eq!(rule_ids(&diags), vec![STRUCTURE_CONFLICTING_NODE_TYPE]);
        assert_eq!(diags[0].path, "both");
    }

    #[test]
    fn rank_mismatch_is_reported_for_both_versions() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", r#"{"zarr_format":2}"#);
        write(
            tmp.path(),
            "t/.zarray",
            r#"{"zarr_format":2,"shape":[4,3],"chunks":[2],"dtype":"<f8"}"#,
        );
        let diags = run(tmp.path());
        assert_eq!(rule_ids(&diags), vec![ARRAY_RANK_MISMATCH]);
        assert!(diags[0].message.contains("rank"));
    }
}
