//! Cloud object-store discovery (`s3://`, `gs://`, `az://`, …) via the
//! `object_store` crate.
//!
//! Unlike plain HTTP, object stores support listing, so discovery walks the
//! prefix and collects every recognized metadata document — the same coverage
//! as the local backend.
//!
//! Credentials come from the environment (env vars, shared config/profile, or
//! instance metadata). By default access falls back to anonymous when
//! credentialed access fails, so public buckets work out of the box;
//! [`StoreOptions::anonymous`] forces anonymous access up front. `object_store`
//! is async, so calls run on a small blocking Tokio runtime.

use std::sync::Arc;

use futures::StreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::azure::MicrosoftAzureBuilder;
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, ObjectStoreExt};
use url::Url;

use crate::scanner::{MetadataRole, RawMetadataFile, ScanError, StoreOptions, StoreScan};

/// Scan a cloud object store rooted at `target` (an `s3://`/`gs://`/`az://` URL).
pub(crate) fn scan(target: &str, options: &StoreOptions) -> Result<StoreScan, ScanError> {
    let url = Url::parse(target).map_err(|err| remote_error(target, err))?;
    let prefix = ObjectPath::from_url_path(url.path()).map_err(|err| remote_error(target, err))?;
    let runtime = tokio::runtime::Runtime::new().map_err(|err| remote_error(target, err))?;

    let files = if options.anonymous {
        let store = build_store(&url, target, true)?;
        runtime.block_on(discover(store, &prefix, target))?
    } else {
        // Prefer credentialed access, but fall back to anonymous so that public
        // buckets are readable without any configured credentials.
        let store = build_store(&url, target, false)?;
        match runtime.block_on(discover(store, &prefix, target)) {
            Ok(files) => files,
            Err(credentialed_err) => {
                let anonymous = build_store(&url, target, true)?;
                runtime
                    .block_on(discover(anonymous, &prefix, target))
                    .map_err(|_| credentialed_err)?
            }
        }
    };

    Ok(StoreScan {
        root: target.to_string(),
        files,
    })
}

/// Build an object store for `url`, optionally anonymous (no request signing).
fn build_store(
    url: &Url,
    target: &str,
    anonymous: bool,
) -> Result<Arc<dyn ObjectStore>, ScanError> {
    let store: Arc<dyn ObjectStore> = match url.scheme() {
        "s3" | "s3a" => {
            let mut builder = AmazonS3Builder::from_env().with_url(target);
            if anonymous {
                builder = builder.with_skip_signature(true);
            }
            Arc::new(builder.build().map_err(|err| remote_error(target, err))?)
        }
        "gs" | "gcs" => {
            // object_store reads public GCS objects without credentials; there
            // is no separate anonymous toggle to apply here.
            let builder = GoogleCloudStorageBuilder::from_env().with_url(target);
            Arc::new(builder.build().map_err(|err| remote_error(target, err))?)
        }
        "az" | "azure" | "abfs" | "abfss" => {
            let mut builder = MicrosoftAzureBuilder::from_env().with_url(target);
            if anonymous {
                builder = builder.with_skip_signature(true);
            }
            Arc::new(builder.build().map_err(|err| remote_error(target, err))?)
        }
        other => return Err(ScanError::UnsupportedScheme(other.to_string())),
    };
    Ok(store)
}

/// List the store under `prefix` and collect every recognized metadata document.
async fn discover(
    store: Arc<dyn ObjectStore>,
    prefix: &ObjectPath,
    root: &str,
) -> Result<Vec<RawMetadataFile>, ScanError> {
    let prefix_str = prefix.as_ref().to_string();
    let mut listing = store.list(Some(prefix));
    let mut files = Vec::new();

    while let Some(meta) = listing.next().await {
        let meta = meta.map_err(|err| remote_error(root, err))?;
        let full = meta.location.as_ref();
        let Some(rel_display) = relativize(&prefix_str, full) else {
            continue;
        };
        let file_name = rel_display.rsplit('/').next().unwrap_or(&rel_display);
        let Some(role) = MetadataRole::from_file_name(file_name) else {
            continue; // ignore chunk data, .zattrs, .zmetadata, etc.
        };
        let location = rel_display
            .rsplit_once('/')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or_default();
        let bytes = store
            .get(&meta.location)
            .await
            .map_err(|err| remote_error(root, err))?
            .bytes()
            .await
            .map_err(|err| remote_error(root, err))?
            .to_vec();
        files.push(RawMetadataFile {
            role,
            source: format!("{}/{}", root.trim_end_matches('/'), rel_display),
            rel_display,
            location,
            bytes,
        });
    }

    files.sort_by(|a, b| a.rel_display.cmp(&b.rel_display));
    Ok(files)
}

/// Return `full` relative to `prefix`, or `None` if `full` is not a proper child
/// of the prefix directory (enforcing a `/` boundary).
fn relativize(prefix: &str, full: &str) -> Option<String> {
    if prefix.is_empty() {
        return Some(full.to_string());
    }
    full.strip_prefix(prefix)?
        .strip_prefix('/')
        .map(str::to_string)
}

fn remote_error(url: &str, err: impl std::fmt::Display) -> ScanError {
    ScanError::Remote {
        url: url.to_string(),
        message: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::memory::InMemory;
    use object_store::{ObjectStoreExt, PutPayload};

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Runtime::new().unwrap().block_on(future)
    }

    fn store_with(entries: &[(&str, &str)]) -> Arc<dyn ObjectStore> {
        let store = InMemory::new();
        block_on(async {
            for (path, body) in entries {
                store
                    .put(
                        &ObjectPath::from(*path),
                        PutPayload::from(body.as_bytes().to_vec()),
                    )
                    .await
                    .unwrap();
            }
        });
        Arc::new(store)
    }

    fn displays(files: &[RawMetadataFile]) -> Vec<String> {
        files.iter().map(|f| f.rel_display.clone()).collect()
    }

    #[test]
    fn discovers_metadata_under_prefix_and_ignores_chunks() {
        let store = store_with(&[
            ("data.zarr/.zgroup", r#"{"zarr_format":2}"#),
            ("data.zarr/temperature/.zarray", r#"{"zarr_format":2}"#),
            ("data.zarr/temperature/0.0", "chunk-bytes-ignored"),
            ("data.zarr/stations/.zgroup", r#"{"zarr_format":2}"#),
            ("data.zarr/v3/zarr.json", r#"{"zarr_format":3}"#),
        ]);
        let prefix = ObjectPath::from("data.zarr");
        let files = block_on(discover(store, &prefix, "s3://bucket/data.zarr")).unwrap();

        assert_eq!(
            displays(&files),
            vec![
                ".zgroup",
                "stations/.zgroup",
                "temperature/.zarray",
                "v3/zarr.json",
            ]
        );
        let temp = files
            .iter()
            .find(|f| f.rel_display == "temperature/.zarray")
            .unwrap();
        assert_eq!(temp.location, "temperature");
        assert_eq!(temp.source, "s3://bucket/data.zarr/temperature/.zarray");
    }

    #[test]
    fn sibling_prefix_is_not_matched() {
        // A "data.zarr2" object must not be treated as a child of "data.zarr".
        let store = store_with(&[
            ("data.zarr/.zgroup", r#"{"zarr_format":2}"#),
            ("data.zarr2/.zgroup", r#"{"zarr_format":2}"#),
        ]);
        let prefix = ObjectPath::from("data.zarr");
        let files = block_on(discover(store, &prefix, "s3://bucket/data.zarr")).unwrap();
        assert_eq!(displays(&files), vec![".zgroup"]);
    }

    #[test]
    fn empty_prefix_is_unrecognized() {
        let store = store_with(&[("other/file.txt", "hi")]);
        let prefix = ObjectPath::from("data.zarr");
        let files = block_on(discover(store, &prefix, "s3://bucket/data.zarr")).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn relativize_enforces_boundary() {
        assert_eq!(
            relativize("a/store.zarr", "a/store.zarr/x/.zarray"),
            Some("x/.zarray".to_string())
        );
        assert_eq!(relativize("store.zarr", "store.zarr2/.zgroup"), None);
        assert_eq!(relativize("store.zarr", "store.zarr"), None);
        assert_eq!(
            relativize("", "store.zarr/.zgroup"),
            Some("store.zarr/.zgroup".to_string())
        );
    }
}
