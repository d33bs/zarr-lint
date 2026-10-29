//! Remote store discovery over HTTP(S).
//!
//! HTTP offers no directory listing, so remote discovery cannot walk a tree the
//! way the local backend does. Instead it relies on **consolidated metadata**:
//!
//! 1. Fetch `<root>/.zmetadata` (Zarr v2 consolidated metadata). If present, it
//!    lists every node's metadata in a single document — the whole store is
//!    discovered in one request.
//! 2. Otherwise fetch `<root>/zarr.json` (Zarr v3). If it carries
//!    `consolidated_metadata`, expand it; otherwise the root node alone is
//!    discovered.
//! 3. Otherwise probe the Zarr v2 root markers `<root>/.zgroup` and
//!    `<root>/.zarray`.
//!
//! Without consolidated metadata a plain HTTP store exposes only its root node,
//! because children cannot be enumerated. Public object stores are reachable
//! through their `https://` endpoints; `s3://`-style access with credentials is
//! out of scope.

use serde_json::Value;

use crate::scanner::{MetadataRole, RawMetadataFile, ScanError, StoreScan};

/// Fetches bytes for a URL. `Ok(None)` means the resource was absent (HTTP 404).
pub(crate) trait Fetcher {
    fn get(&self, url: &str) -> Result<Option<Vec<u8>>, ScanError>;
}

/// A [`Fetcher`] backed by a blocking HTTP client.
struct HttpFetcher;

impl Fetcher for HttpFetcher {
    fn get(&self, url: &str) -> Result<Option<Vec<u8>>, ScanError> {
        match ureq::get(url).call() {
            Ok(mut response) => {
                let bytes = response
                    .body_mut()
                    .read_to_vec()
                    .map_err(|err| remote_error(url, err))?;
                Ok(Some(bytes))
            }
            Err(ureq::Error::StatusCode(404)) => Ok(None),
            Err(err) => Err(remote_error(url, err)),
        }
    }
}

fn remote_error(url: &str, err: impl std::fmt::Display) -> ScanError {
    ScanError::Remote {
        url: url.to_string(),
        message: err.to_string(),
    }
}

/// Scan a remote store rooted at `root` (an `http(s)://` URL).
pub(crate) fn scan(root: &str) -> Result<StoreScan, ScanError> {
    scan_with(root, &HttpFetcher)
}

fn scan_with(root: &str, fetcher: &dyn Fetcher) -> Result<StoreScan, ScanError> {
    // 1. Zarr v2 consolidated metadata: authoritative and complete.
    if let Some(bytes) = fetcher.get(&join(root, ".zmetadata"))? {
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            let files = expand_v2_consolidated(&value, root)?;
            if !files.is_empty() {
                return Ok(StoreScan {
                    root: root.to_string(),
                    files,
                });
            }
        }
    }

    // 2. Zarr v3 root, possibly carrying consolidated metadata.
    if let Some(bytes) = fetcher.get(&join(root, "zarr.json"))? {
        let files = expand_v3_root(&bytes, root)?;
        return Ok(StoreScan {
            root: root.to_string(),
            files,
        });
    }

    // 3. Zarr v2 root markers (root node only; children are not enumerable).
    let mut files = Vec::new();
    for (name, role) in [
        (".zgroup", MetadataRole::V2Group),
        (".zarray", MetadataRole::V2Array),
    ] {
        if let Some(bytes) = fetcher.get(&join(root, name))? {
            files.push(RawMetadataFile {
                role,
                source: join(root, name),
                rel_display: name.to_string(),
                location: String::new(),
                bytes,
            });
        }
    }

    Ok(StoreScan {
        root: root.to_string(),
        files,
    })
}

/// Expand Zarr v2 consolidated metadata (`.zmetadata`) into node records.
fn expand_v2_consolidated(value: &Value, root: &str) -> Result<Vec<RawMetadataFile>, ScanError> {
    let Some(metadata) = value.get("metadata").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut files = Vec::new();
    for (key, node_metadata) in metadata {
        // Keys are file-relative paths such as ".zgroup" or "grp/.zarray".
        let file_name = key.rsplit('/').next().unwrap_or(key);
        let Some(role) = MetadataRole::from_file_name(file_name) else {
            continue; // ignore .zattrs and other non-node entries
        };
        let location = key
            .rsplit_once('/')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_default();
        files.push(RawMetadataFile {
            role,
            source: join(root, key),
            rel_display: key.clone(),
            location,
            bytes: to_bytes(node_metadata),
        });
    }
    Ok(files)
}

/// Build node records from a Zarr v3 root `zarr.json`, expanding consolidated
/// metadata when present.
fn expand_v3_root(root_bytes: &[u8], root: &str) -> Result<Vec<RawMetadataFile>, ScanError> {
    let mut files = vec![RawMetadataFile {
        role: MetadataRole::V3Node,
        source: join(root, "zarr.json"),
        rel_display: "zarr.json".to_string(),
        location: String::new(),
        bytes: root_bytes.to_vec(),
    }];

    if let Ok(value) = serde_json::from_slice::<Value>(root_bytes) {
        if let Some(consolidated) = value
            .get("consolidated_metadata")
            .and_then(|c| c.get("metadata"))
            .and_then(Value::as_object)
        {
            for (name, node_metadata) in consolidated {
                if name.is_empty() {
                    continue; // the root node is already included above
                }
                files.push(RawMetadataFile {
                    role: MetadataRole::V3Node,
                    source: join(root, &format!("{name}/zarr.json")),
                    rel_display: format!("{name}/zarr.json"),
                    location: name.clone(),
                    bytes: to_bytes(node_metadata),
                });
            }
        }
    }

    Ok(files)
}

/// Serialize a parsed metadata value back to bytes for the normal parse path.
/// Serializing a [`Value`] is infallible for the JSON data model.
fn to_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

/// Join a store root and a store-relative key into a URL.
fn join(root: &str, key: &str) -> String {
    if root.ends_with('/') {
        format!("{root}{key}")
    } else {
        format!("{root}/{key}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockFetcher {
        responses: HashMap<String, Vec<u8>>,
    }

    impl MockFetcher {
        fn new(entries: &[(&str, &str)]) -> Self {
            Self {
                responses: entries
                    .iter()
                    .map(|(url, body)| (url.to_string(), body.as_bytes().to_vec()))
                    .collect(),
            }
        }
    }

    impl Fetcher for MockFetcher {
        fn get(&self, url: &str) -> Result<Option<Vec<u8>>, ScanError> {
            Ok(self.responses.get(url).cloned())
        }
    }

    fn displays(scan: &StoreScan) -> Vec<String> {
        let mut d: Vec<String> = scan.files.iter().map(|f| f.rel_display.clone()).collect();
        d.sort();
        d
    }

    #[test]
    fn v2_consolidated_discovers_all_nodes() {
        let consolidated = r#"{
            "zarr_consolidated_format": 1,
            "metadata": {
                ".zgroup": {"zarr_format": 2},
                ".zattrs": {"note": "ignored"},
                "temperature/.zarray": {"zarr_format": 2, "shape": [4, 3], "chunks": [2, 2]},
                "stations/.zgroup": {"zarr_format": 2},
                "stations/elevation/.zarray": {"zarr_format": 2, "shape": [2], "chunks": [2]}
            }
        }"#;
        let fetcher = MockFetcher::new(&[("https://h/s.zarr/.zmetadata", consolidated)]);
        let scan = scan_with("https://h/s.zarr", &fetcher).unwrap();
        assert_eq!(
            displays(&scan),
            vec![
                ".zgroup",
                "stations/.zgroup",
                "stations/elevation/.zarray",
                "temperature/.zarray",
            ]
        );
        let temp = scan
            .files
            .iter()
            .find(|f| f.rel_display == "temperature/.zarray")
            .unwrap();
        assert_eq!(temp.location, "temperature");
        assert_eq!(temp.source, "https://h/s.zarr/temperature/.zarray");
    }

    #[test]
    fn v3_root_without_consolidation_yields_root_only() {
        let root = r#"{"zarr_format": 3, "node_type": "group"}"#;
        let fetcher = MockFetcher::new(&[("https://h/s.zarr/zarr.json", root)]);
        let scan = scan_with("https://h/s.zarr", &fetcher).unwrap();
        assert_eq!(displays(&scan), vec!["zarr.json"]);
        assert_eq!(scan.files[0].role, MetadataRole::V3Node);
    }

    #[test]
    fn v3_consolidated_expands_children() {
        let root = r#"{
            "zarr_format": 3,
            "node_type": "group",
            "consolidated_metadata": {
                "kind": "inline",
                "metadata": {
                    "temp": {"zarr_format": 3, "node_type": "array", "shape": [2]}
                }
            }
        }"#;
        let fetcher = MockFetcher::new(&[("https://h/s.zarr/zarr.json", root)]);
        let scan = scan_with("https://h/s.zarr", &fetcher).unwrap();
        assert_eq!(displays(&scan), vec!["temp/zarr.json", "zarr.json"]);
    }

    #[test]
    fn v2_root_marker_fallback() {
        let fetcher = MockFetcher::new(&[("https://h/s.zarr/.zgroup", r#"{"zarr_format": 2}"#)]);
        let scan = scan_with("https://h/s.zarr", &fetcher).unwrap();
        assert_eq!(displays(&scan), vec![".zgroup"]);
    }

    #[test]
    fn nothing_found_is_unrecognized() {
        let fetcher = MockFetcher::new(&[]);
        let scan = scan_with("https://h/s.zarr", &fetcher).unwrap();
        assert!(!scan.is_recognized());
    }

    #[test]
    fn trailing_slash_root_joins_cleanly() {
        let fetcher = MockFetcher::new(&[("https://h/s.zarr/.zgroup", r#"{"zarr_format": 2}"#)]);
        let scan = scan_with("https://h/s.zarr/", &fetcher).unwrap();
        assert_eq!(scan.files[0].source, "https://h/s.zarr/.zgroup");
    }

    // Real end-to-end HTTP round-trip through the ureq-backed fetcher.
    #[test]
    fn http_fetcher_reads_consolidated_over_the_wire() {
        let consolidated = r#"{"zarr_consolidated_format":1,"metadata":{
            ".zgroup":{"zarr_format":2},
            "t/.zarray":{"zarr_format":2,"shape":[2,2],"chunks":[2,2]}
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

        let url = format!("http://127.0.0.1:{port}/s.zarr");
        let scan = scan(&url).unwrap();
        handle.join().unwrap();

        assert_eq!(displays(&scan), vec![".zgroup", "t/.zarray"]);
    }
}
