//! Local filesystem store discovery.
//!
//! The scanner walks a directory tree and collects the Zarr metadata files it
//! recognizes by name. It performs no JSON parsing and applies no rules; it
//! only answers the question "where are the metadata documents, and which Zarr
//! flavor does each file name imply?". Parsing and validation happen later in
//! [`crate::model`] and [`crate::rule`].

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

/// Zarr v2 group marker file name.
pub const V2_GROUP_FILE: &str = ".zgroup";
/// Zarr v2 array marker file name.
pub const V2_ARRAY_FILE: &str = ".zarray";
/// Zarr v3 node marker file name (group or array; disambiguated by `node_type`).
pub const V3_NODE_FILE: &str = "zarr.json";

/// The kind of metadata file, as implied purely by its name.
///
/// For Zarr v2 the file name fully determines whether the node is a group or an
/// array. For Zarr v3 a single `zarr.json` serves both, so the concrete node
/// kind is only known after parsing the `node_type` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataRole {
    /// A Zarr v2 `.zgroup` file.
    V2Group,
    /// A Zarr v2 `.zarray` file.
    V2Array,
    /// A Zarr v3 `zarr.json` file.
    V3Node,
}

impl MetadataRole {
    /// Classify a file name into a metadata role, if it is a recognized marker.
    pub fn from_file_name(name: &str) -> Option<Self> {
        match name {
            V2_GROUP_FILE => Some(MetadataRole::V2Group),
            V2_ARRAY_FILE => Some(MetadataRole::V2Array),
            V3_NODE_FILE => Some(MetadataRole::V3Node),
            _ => None,
        }
    }

    /// The Zarr format version implied by the file name.
    pub fn version(self) -> crate::model::ZarrVersion {
        match self {
            MetadataRole::V2Group | MetadataRole::V2Array => crate::model::ZarrVersion::V2,
            MetadataRole::V3Node => crate::model::ZarrVersion::V3,
        }
    }
}

/// A recognized metadata file together with its raw contents and locations.
#[derive(Debug, Clone)]
pub struct RawMetadataFile {
    /// The role implied by the file name.
    pub role: MetadataRole,
    /// Absolute (or as-supplied) filesystem path to the metadata file.
    pub fs_path: PathBuf,
    /// Store-relative path of the metadata file, using `/` separators
    /// (for example `stations/elevation/.zarray`, or `.zgroup` at the root).
    pub rel_display: String,
    /// Store-relative path of the *node* (the directory containing the file),
    /// using `/` separators. Empty string for a node at the store root.
    pub location: String,
    /// Raw bytes of the metadata file.
    pub bytes: Vec<u8>,
}

/// The result of scanning a store: its root and every recognized metadata file.
#[derive(Debug, Clone)]
pub struct StoreScan {
    /// The store root as supplied to [`scan_store`].
    pub root: PathBuf,
    /// Every recognized metadata file discovered under the root, in a
    /// deterministic (sorted) order.
    pub files: Vec<RawMetadataFile>,
}

impl StoreScan {
    /// Whether any recognizable Zarr metadata was found.
    pub fn is_recognized(&self) -> bool {
        !self.files.is_empty()
    }
}

/// Errors that prevent a store from being scanned at all.
///
/// These correspond to store-access failures (CLI exit code `3`) rather than
/// lint findings.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// The supplied path does not exist.
    #[error("store path does not exist: {0}")]
    NotFound(PathBuf),
    /// The supplied path exists but is not a directory.
    #[error("store path is not a directory: {0}")]
    NotADirectory(PathBuf),
    /// An I/O error occurred while reading a specific path.
    #[error("failed to read {path}: {source}")]
    Io {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// An error occurred while walking the directory tree.
    #[error("failed to traverse store: {0}")]
    Walk(#[source] walkdir::Error),
}

/// Discover the Zarr metadata files under `root`.
///
/// Returns [`ScanError`] only for store-access problems (missing path, not a
/// directory, unreadable file). A directory that simply contains no Zarr
/// metadata is *not* an error here; it produces an empty [`StoreScan`] whose
/// [`StoreScan::is_recognized`] is `false`, which the rule engine reports as
/// `structure/unrecognized-store`.
pub fn scan_store(root: &Path) -> Result<StoreScan, ScanError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            ScanError::NotFound(root.to_path_buf())
        } else {
            ScanError::Io {
                path: root.to_path_buf(),
                source,
            }
        }
    })?;

    // Resolve through a symlinked root, but require the target to be a directory.
    if !root.is_dir() {
        // `is_dir` follows symlinks; combine with the earlier stat so that a
        // dangling symlink surfaces as NotFound rather than NotADirectory.
        if metadata.file_type().is_symlink() && !root.exists() {
            return Err(ScanError::NotFound(root.to_path_buf()));
        }
        return Err(ScanError::NotADirectory(root.to_path_buf()));
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(root).sort_by_file_name() {
        let entry = entry.map_err(ScanError::Walk)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str() else {
            continue;
        };
        let Some(role) = MetadataRole::from_file_name(name) else {
            continue;
        };

        let fs_path = entry.path().to_path_buf();
        let bytes = std::fs::read(&fs_path).map_err(|source| ScanError::Io {
            path: fs_path.clone(),
            source,
        })?;

        let rel_display = relative_display(root, &fs_path);
        let location = rel_display
            .rsplit_once('/')
            .map(|(dir, _file)| dir.to_string())
            .unwrap_or_default();

        files.push(RawMetadataFile {
            role,
            fs_path,
            rel_display,
            location,
            bytes,
        });
    }

    Ok(StoreScan {
        root: root.to_path_buf(),
        files,
    })
}

/// Render `path` relative to `root` using `/` separators.
///
/// Falls back to the file's own name if `path` is not under `root` (which
/// should not happen for walked entries, but keeps the function total).
fn relative_display(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn missing_path_is_not_found() {
        let err = scan_store(Path::new("/definitely/not/here/zarr-lint")).unwrap_err();
        assert!(matches!(err, ScanError::NotFound(_)));
    }

    #[test]
    fn file_path_is_not_a_directory() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("plain.txt");
        fs::write(&file, "hi").unwrap();
        let err = scan_store(&file).unwrap_err();
        assert!(matches!(err, ScanError::NotADirectory(_)));
    }

    #[test]
    fn empty_directory_is_unrecognized_but_not_an_error() {
        let tmp = TempDir::new().unwrap();
        let scan = scan_store(tmp.path()).unwrap();
        assert!(!scan.is_recognized());
        assert!(scan.files.is_empty());
    }

    #[test]
    fn discovers_v2_and_v3_markers_and_ignores_chunks() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".zgroup", r#"{"zarr_format":2}"#);
        write(tmp.path(), "arr/.zarray", r#"{"zarr_format":2}"#);
        write(tmp.path(), "arr/0.0", "chunkbytes"); // must be ignored
        write(tmp.path(), "v3/zarr.json", r#"{"zarr_format":3}"#);
        write(tmp.path(), "v3/c/0/0", "chunkbytes"); // must be ignored

        let scan = scan_store(tmp.path()).unwrap();
        let displays: Vec<&str> = scan.files.iter().map(|f| f.rel_display.as_str()).collect();
        assert_eq!(displays, vec![".zgroup", "arr/.zarray", "v3/zarr.json"]);

        let root_group = &scan.files[0];
        assert_eq!(root_group.role, MetadataRole::V2Group);
        assert_eq!(root_group.location, "");

        let arr = &scan.files[1];
        assert_eq!(arr.role, MetadataRole::V2Array);
        assert_eq!(arr.location, "arr");

        let v3 = &scan.files[2];
        assert_eq!(v3.role, MetadataRole::V3Node);
        assert_eq!(v3.location, "v3");
    }
}
