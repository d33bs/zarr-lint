//! Store discovery for local and remote Zarr stores.
//!
//! Discovery finds the Zarr metadata documents that make up a store and returns
//! them, with their raw bytes, for parsing and linting. Two backends are
//! supported:
//!
//! * **Local** (a filesystem path): the directory tree is walked and every
//!   recognized metadata file is collected.
//! * **Remote** (an `http://` or `https://` URL): see [`crate::remote`]. Because
//!   HTTP offers no directory listing, remote discovery relies on consolidated
//!   metadata, falling back to the root node.
//!
//! Neither backend parses JSON or applies rules; that happens later in
//! [`crate::model`] and [`crate::rule`].

use std::path::Path;

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

/// A recognized metadata document together with its raw contents and locations.
#[derive(Debug, Clone)]
pub struct RawMetadataFile {
    /// The role implied by the file name.
    pub role: MetadataRole,
    /// Where the document came from: a filesystem path or a URL, for display
    /// and provenance.
    pub source: String,
    /// Store-relative path of the metadata document, using `/` separators
    /// (for example `stations/elevation/.zarray`, or `.zgroup` at the root).
    pub rel_display: String,
    /// Store-relative path of the *node* (the directory containing the file),
    /// using `/` separators. Empty string for a node at the store root.
    pub location: String,
    /// Raw bytes of the metadata document.
    pub bytes: Vec<u8>,
}

/// The result of scanning a store: its root and every recognized metadata file.
#[derive(Debug, Clone)]
pub struct StoreScan {
    /// The store root as supplied to [`scan_store`] (a path or URL).
    pub root: String,
    /// Every recognized metadata document discovered under the root, in a
    /// deterministic (sorted) order.
    pub files: Vec<RawMetadataFile>,
}

impl StoreScan {
    /// Whether any recognizable Zarr metadata was found.
    pub fn is_recognized(&self) -> bool {
        !self.files.is_empty()
    }
}

/// Options controlling how a store is accessed.
#[derive(Debug, Clone, Default)]
pub struct StoreOptions {
    /// Access cloud object stores anonymously (no credentials or request
    /// signing). When `false`, credentials are taken from the environment and
    /// access falls back to anonymous for public data.
    pub anonymous: bool,
}

/// Errors that prevent a store from being scanned at all.
///
/// These correspond to store-access failures (CLI exit code `3`) rather than
/// lint findings.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// The supplied path does not exist.
    #[error("store path does not exist: {0}")]
    NotFound(String),
    /// The supplied path exists but is not a directory.
    #[error("store path is not a directory: {0}")]
    NotADirectory(String),
    /// An I/O error occurred while reading a specific path.
    #[error("failed to read {path}: {source}")]
    Io {
        /// The path that could not be read.
        path: String,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// An error occurred while walking the directory tree.
    #[error("failed to traverse store: {0}")]
    Walk(#[source] walkdir::Error),
    /// A remote request failed (network error or non-404 HTTP status).
    #[error("failed to read remote store {url}: {message}")]
    Remote {
        /// The URL that could not be read.
        url: String,
        /// A description of the failure.
        message: String,
    },
    /// The URL scheme is recognized but not supported.
    #[error("unsupported URL scheme `{0}://`: only http and https are supported")]
    UnsupportedScheme(String),
}

/// Discover the Zarr metadata documents in the store at `target`.
///
/// `target` may be a local filesystem path, an `http(s)://` URL, or a cloud
/// object-store URL (`s3://`, `gs://`, `az://`, …). Returns [`ScanError`] only
/// for store-access problems; a location that simply contains no Zarr metadata
/// is *not* an error here — it produces an empty [`StoreScan`] whose
/// [`StoreScan::is_recognized`] is `false`, which the rule engine reports as
/// `structure/unrecognized-store`.
pub fn scan_store(target: &str, options: &StoreOptions) -> Result<StoreScan, ScanError> {
    match url_scheme(target).as_deref() {
        None => scan_local(Path::new(target)),
        Some("http") | Some("https") => crate::remote::scan(target),
        Some("s3") | Some("s3a") | Some("gs") | Some("gcs") | Some("az") | Some("azure")
        | Some("abfs") | Some("abfss") => crate::cloud::scan(target, options),
        Some(other) => Err(ScanError::UnsupportedScheme(other.to_string())),
    }
}

/// The lowercase URL scheme of `target` (e.g. `https`), if it looks like a URL.
///
/// Matches `^[a-zA-Z][a-zA-Z0-9+.-]*://`, so Windows drive paths like `C:\data`
/// (which have no `//`) are treated as local paths, not URLs.
fn url_scheme(target: &str) -> Option<String> {
    let bytes = target.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let mut end = 1;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'+' | b'.' | b'-'))
    {
        end += 1;
    }
    if target[end..].starts_with("://") {
        Some(target[..end].to_ascii_lowercase())
    } else {
        None
    }
}

fn scan_local(root: &Path) -> Result<StoreScan, ScanError> {
    let root_display = root.display().to_string();
    let metadata = std::fs::symlink_metadata(root).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            ScanError::NotFound(root_display.clone())
        } else {
            ScanError::Io {
                path: root_display.clone(),
                source,
            }
        }
    })?;

    // Resolve through a symlinked root, but require the target to be a directory.
    if !root.is_dir() {
        // `is_dir` follows symlinks; combine with the earlier stat so that a
        // dangling symlink surfaces as NotFound rather than NotADirectory.
        if metadata.file_type().is_symlink() && !root.exists() {
            return Err(ScanError::NotFound(root_display));
        }
        return Err(ScanError::NotADirectory(root_display));
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

        let fs_path = entry.path();
        let bytes = std::fs::read(fs_path).map_err(|source| ScanError::Io {
            path: fs_path.display().to_string(),
            source,
        })?;

        let rel_display = relative_display(root, fs_path);
        let location = rel_display
            .rsplit_once('/')
            .map(|(dir, _file)| dir.to_string())
            .unwrap_or_default();

        files.push(RawMetadataFile {
            role,
            source: fs_path.display().to_string(),
            rel_display,
            location,
            bytes,
        });
    }

    Ok(StoreScan {
        root: root_display,
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

    fn scan_path(path: &Path) -> Result<StoreScan, ScanError> {
        scan_store(path.to_str().unwrap(), &StoreOptions::default())
    }

    #[test]
    fn url_scheme_detection() {
        assert_eq!(url_scheme("https://example.com/x"), Some("https".into()));
        assert_eq!(url_scheme("HTTP://example.com/x"), Some("http".into()));
        assert_eq!(url_scheme("s3://bucket/x"), Some("s3".into()));
        assert_eq!(url_scheme("/local/path/store.zarr"), None);
        assert_eq!(url_scheme("relative/store.zarr"), None);
        assert_eq!(url_scheme(r"C:\data\store.zarr"), None);
    }

    #[test]
    fn unsupported_scheme_is_an_error() {
        let err = scan_store("ftp://host/store.zarr", &StoreOptions::default()).unwrap_err();
        assert!(matches!(err, ScanError::UnsupportedScheme(s) if s == "ftp"));
    }

    #[test]
    fn missing_path_is_not_found() {
        let err =
            scan_store("/definitely/not/here/zarr-lint", &StoreOptions::default()).unwrap_err();
        assert!(matches!(err, ScanError::NotFound(_)));
    }

    #[test]
    fn file_path_is_not_a_directory() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("plain.txt");
        fs::write(&file, "hi").unwrap();
        let err = scan_path(&file).unwrap_err();
        assert!(matches!(err, ScanError::NotADirectory(_)));
    }

    #[test]
    fn empty_directory_is_unrecognized_but_not_an_error() {
        let tmp = TempDir::new().unwrap();
        let scan = scan_path(tmp.path()).unwrap();
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

        let scan = scan_path(tmp.path()).unwrap();
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
