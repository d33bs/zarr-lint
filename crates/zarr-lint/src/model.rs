//! The minimal normalized Zarr model and metadata field extraction.
//!
//! The model deliberately preserves the raw JSON of each metadata document and
//! extracts only the handful of fields the rules need. This keeps the linter
//! honest about how much of the Zarr specification it currently understands,
//! while leaving room to grow into strongly typed v2 and v3 metadata structures.

use serde_json::Value;

use crate::scanner::{MetadataRole, RawMetadataFile, StoreScan};

/// The Zarr format version a node was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZarrVersion {
    /// Zarr version 2 (`.zgroup` / `.zarray`).
    V2,
    /// Zarr version 3 (`zarr.json`).
    V3,
}

impl ZarrVersion {
    /// A short display label (`v2` / `v3`).
    pub fn as_str(self) -> &'static str {
        match self {
            ZarrVersion::V2 => "v2",
            ZarrVersion::V3 => "v3",
        }
    }
}

impl std::fmt::Display for ZarrVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a node is a group (a container) or an array (holds chunked data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A Zarr group.
    Group,
    /// A Zarr array.
    Array,
}

impl NodeKind {
    /// A short display label (`group` / `array`).
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Group => "group",
            NodeKind::Array => "array",
        }
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A metadata document that parsed as JSON, retained with its raw [`Value`].
///
/// A `ParsedMetadata` may still be *invalid* Zarr (missing fields, bad ranks,
/// unsupported versions); those problems are surfaced by the rule engine. What
/// it guarantees is only that the bytes were syntactically valid JSON.
#[derive(Debug, Clone)]
pub struct ParsedMetadata {
    /// The role implied by the metadata file name.
    pub role: MetadataRole,
    /// The Zarr version implied by the metadata file name.
    pub version: ZarrVersion,
    /// Store-relative node path (empty string for the store root).
    pub location: String,
    /// Store-relative display path of the metadata file.
    pub rel_display: String,
    /// Where the document came from: a filesystem path or a URL.
    pub source: String,
    /// The parsed JSON value.
    pub value: Value,
}

impl ParsedMetadata {
    /// The declared `zarr_format`, if present and an integer.
    pub fn declared_format(&self) -> Option<i64> {
        self.value.get("zarr_format").and_then(Value::as_i64)
    }

    /// The declared `node_type` (Zarr v3), if present and a string.
    pub fn node_type(&self) -> Option<&str> {
        self.value.get("node_type").and_then(Value::as_str)
    }

    /// Resolve the concrete node kind.
    ///
    /// For Zarr v2 the kind is fully determined by the file name. For Zarr v3 it
    /// comes from `node_type`; a missing or unrecognized `node_type` yields
    /// `None` (and is separately reported as a missing required field).
    pub fn kind(&self) -> Option<NodeKind> {
        match self.role {
            MetadataRole::V2Group => Some(NodeKind::Group),
            MetadataRole::V2Array => Some(NodeKind::Array),
            MetadataRole::V3Node => match self.node_type() {
                Some("group") => Some(NodeKind::Group),
                Some("array") => Some(NodeKind::Array),
                _ => None,
            },
        }
    }

    /// The array shape as a JSON array, if the `shape` field is present and is
    /// an array. Applies to array nodes in both Zarr versions.
    pub fn shape_dims(&self) -> Option<&Vec<Value>> {
        self.value.get("shape").and_then(Value::as_array)
    }

    /// The chunk shape as a JSON array, if determinable.
    ///
    /// * Zarr v2 reads `chunks`.
    /// * Zarr v3 reads `chunk_grid.configuration.chunk_shape` (regular grids).
    pub fn chunk_dims(&self) -> Option<&Vec<Value>> {
        match self.version {
            ZarrVersion::V2 => self.value.get("chunks").and_then(Value::as_array),
            ZarrVersion::V3 => self
                .value
                .get("chunk_grid")
                .and_then(|g| g.get("configuration"))
                .and_then(|c| c.get("chunk_shape"))
                .and_then(Value::as_array),
        }
    }

    /// Whether `key` is present with a non-null value.
    ///
    /// An explicit JSON `null` is treated the same as an absent key: it carries
    /// no usable value, so for a *required* field it counts as missing.
    pub fn has_field(&self, key: &str) -> bool {
        matches!(self.value.get(key), Some(value) if !value.is_null())
    }

    /// Whether the field that carries chunk-grid information is present at all.
    ///
    /// Used by the missing-required-field rule: v2 requires `chunks`, v3
    /// requires `chunk_grid`.
    pub fn has_chunk_grid_field(&self) -> bool {
        match self.version {
            ZarrVersion::V2 => self.has_field("chunks"),
            ZarrVersion::V3 => self.has_field("chunk_grid"),
        }
    }

    /// Build the normalized [`ZarrNode`] for this metadata, if the node kind is
    /// determinable. Returns `None` for a v3 node with no usable `node_type`.
    pub fn to_node(&self) -> Option<ZarrNode> {
        let kind = self.kind()?;
        Some(ZarrNode {
            logical_path: self.location.clone(),
            version: self.version,
            kind,
            source: self.source.clone(),
            metadata: self.value.clone(),
        })
    }
}

/// A metadata document whose bytes were not valid JSON.
#[derive(Debug, Clone)]
pub struct ParseFailure {
    /// Store-relative display path of the metadata file.
    pub rel_display: String,
    /// The JSON parser's error message.
    pub message: String,
}

/// The normalized representation of a single Zarr node.
///
/// A minimal model: enough structure for basic checks and inspection, with the
/// raw metadata retained verbatim.
#[derive(Debug, Clone)]
pub struct ZarrNode {
    /// Store-relative logical path (empty string for the store root).
    pub logical_path: String,
    /// The Zarr format version.
    pub version: ZarrVersion,
    /// Whether the node is a group or an array.
    pub kind: NodeKind,
    /// Where the metadata came from: a filesystem path or a URL.
    pub source: String,
    /// The raw metadata JSON.
    pub metadata: Value,
}

/// The parsed contents of a store: nodes that parsed and files that did not.
#[derive(Debug, Clone, Default)]
pub struct LoadedStore {
    /// Metadata documents that parsed as JSON.
    pub parsed: Vec<ParsedMetadata>,
    /// Metadata documents whose bytes were not valid JSON.
    pub parse_failures: Vec<ParseFailure>,
}

/// Parse every discovered metadata file into the loaded model.
///
/// This never fails: unpardseable documents are collected as
/// [`ParseFailure`]s rather than aborting the load.
pub fn load(scan: &StoreScan) -> LoadedStore {
    let mut loaded = LoadedStore::default();
    for file in &scan.files {
        match parse_file(file) {
            Ok(parsed) => loaded.parsed.push(parsed),
            Err(failure) => loaded.parse_failures.push(failure),
        }
    }
    loaded
}

fn parse_file(file: &RawMetadataFile) -> Result<ParsedMetadata, ParseFailure> {
    match serde_json::from_slice::<Value>(&file.bytes) {
        Ok(value) => Ok(ParsedMetadata {
            role: file.role,
            version: file.role.version(),
            location: file.location.clone(),
            rel_display: file.rel_display.clone(),
            source: file.source.clone(),
            value,
        }),
        Err(err) => Err(ParseFailure {
            rel_display: file.rel_display.clone(),
            message: err.to_string(),
        }),
    }
}

/// Render a JSON array of dimensions as a compact `[a, b, c]` string.
pub fn format_dims(dims: &[Value]) -> String {
    let parts: Vec<String> = dims.iter().map(display_dim).collect();
    format!("[{}]", parts.join(", "))
}

fn display_dim(value: &Value) -> String {
    value
        .as_i64()
        .map(|n| n.to_string())
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parsed(role: MetadataRole, value: Value) -> ParsedMetadata {
        ParsedMetadata {
            role,
            version: role.version(),
            location: "n".into(),
            rel_display: "n/meta".into(),
            source: "/store/n/meta".into(),
            value,
        }
    }

    #[test]
    fn v2_array_extracts_shape_and_chunks() {
        let p = parsed(
            MetadataRole::V2Array,
            json!({"zarr_format": 2, "shape": [4, 3], "chunks": [2, 2]}),
        );
        assert_eq!(p.declared_format(), Some(2));
        assert_eq!(p.kind(), Some(NodeKind::Array));
        assert_eq!(p.shape_dims().unwrap().len(), 2);
        assert_eq!(p.chunk_dims().unwrap().len(), 2);
        assert!(p.has_chunk_grid_field());
    }

    #[test]
    fn v3_array_reads_nested_chunk_shape() {
        let p = parsed(
            MetadataRole::V3Node,
            json!({
                "zarr_format": 3,
                "node_type": "array",
                "shape": [3, 2],
                "chunk_grid": {"name": "regular", "configuration": {"chunk_shape": [2, 2]}}
            }),
        );
        assert_eq!(p.kind(), Some(NodeKind::Array));
        assert_eq!(p.chunk_dims().unwrap().len(), 2);
        assert!(p.has_chunk_grid_field());
    }

    #[test]
    fn v3_missing_node_type_has_no_kind() {
        let p = parsed(MetadataRole::V3Node, json!({"zarr_format": 3}));
        assert_eq!(p.kind(), None);
        assert!(p.to_node().is_none());
    }

    #[test]
    fn null_required_field_counts_as_absent() {
        let p = parsed(
            MetadataRole::V3Node,
            json!({"zarr_format": 3, "node_type": "array", "shape": null, "chunk_grid": null}),
        );
        assert!(!p.has_field("shape"));
        assert!(!p.has_chunk_grid_field());
    }

    #[test]
    fn format_dims_is_compact() {
        assert_eq!(format_dims(&[json!(4), json!(3)]), "[4, 3]");
    }
}
