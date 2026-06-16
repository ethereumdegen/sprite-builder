//! S3-compatible object storage (ADR 0001: a thin shared module the domain
//! handlers build on). Backs Docuspaces — a project's files live as plain objects
//! under a per-docuspace key prefix, with no sprite and no worker involved.
//!
//! The bucket is constructed per call from [`Config`]: a `rust-s3` `Bucket` holds
//! no connection pool (each op is an independent HTTPS request), so there's
//! nothing to cache on `AppState`. When any S3 credential is missing the helper
//! returns a clean 400 rather than panicking, so a deploy without object storage
//! still boots and only the Docuspace endpoints degrade.

use s3::creds::Credentials;
use s3::{Bucket, Region};

use crate::config::Config;
use crate::error::{AppError, AppResult};

/// A file/folder entry in a directory listing.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ObjectEntry {
    pub name: String,
    pub is_dir: bool,
    /// Size in bytes (0 for directories).
    pub size: i64,
}

/// Build the configured bucket, or a 400 explaining what's missing. The same
/// error the Docuspace handlers surface when object storage isn't wired up.
pub fn build_bucket(config: &Config) -> AppResult<Box<Bucket>> {
    let endpoint = config
        .s3_endpoint
        .as_deref()
        .ok_or_else(|| missing("S3_ENDPOINT"))?;
    let access_key = config
        .s3_access_key
        .as_deref()
        .ok_or_else(|| missing("S3_ACCESS_KEY"))?;
    let secret_key = config
        .s3_secret_key
        .as_deref()
        .ok_or_else(|| missing("S3_SECRET_KEY"))?;

    let region = Region::Custom {
        region: config.s3_region.clone(),
        endpoint: endpoint.to_string(),
    };
    let credentials = Credentials::new(Some(access_key), Some(secret_key), None, None, None)
        .map_err(|e| AppError::bad_request(format!("invalid S3 credentials: {e}")))?;

    Bucket::new(&config.s3_bucket, region, credentials)
        .map_err(|e| AppError::bad_request(format!("could not open S3 bucket: {e}")))
}

fn missing(var: &str) -> AppError {
    AppError::bad_request(format!(
        "S3 is not configured. Set {var} (and S3_ENDPOINT/S3_ACCESS_KEY/S3_SECRET_KEY/S3_BUCKET)."
    ))
}

/// Whether the S3 response status counts as "object not found" (404). Both an
/// `Ok` response carrying a 404 and an `Err` HTTP failure are normalized here.
fn is_not_found(status: u16) -> bool {
    status == 404
}

/// Fetch an object's bytes. `Ok(None)` means a clean 404 (no such key); any other
/// non-2xx is an internal error.
pub async fn get_bytes(bucket: &Bucket, key: &str) -> AppResult<Option<Vec<u8>>> {
    match bucket.get_object(key).await {
        Ok(resp) => {
            let status = resp.status_code();
            if (200..300).contains(&status) {
                Ok(Some(resp.bytes().to_vec()))
            } else if is_not_found(status) {
                Ok(None)
            } else {
                Err(AppError::bad_request(format!("S3 read failed ({status})")))
            }
        }
        Err(s3::error::S3Error::HttpFailWithBody(status, _)) if is_not_found(status) => Ok(None),
        Err(e) => Err(AppError::bad_request(format!("S3 read failed: {e}"))),
    }
}

/// Write bytes to a key with an explicit content type.
pub async fn put_bytes(
    bucket: &Bucket,
    key: &str,
    bytes: &[u8],
    content_type: &str,
) -> AppResult<()> {
    bucket
        .put_object_with_content_type(key, bytes, content_type)
        .await
        .map_err(|e| AppError::bad_request(format!("S3 write failed: {e}")))?;
    Ok(())
}

/// Delete a single object. Idempotent — deleting a missing key is not an error.
pub async fn delete_object(bucket: &Bucket, key: &str) -> AppResult<()> {
    bucket
        .delete_object(key)
        .await
        .map_err(|e| AppError::bad_request(format!("S3 delete failed: {e}")))?;
    Ok(())
}

/// List the immediate children of a folder `prefix` (which must end in `/`, or be
/// empty for the root). Uses a `/` delimiter so sub-folders come back as
/// directory entries rather than every descendant key. The `.keep` marker (used to
/// materialize empty folders) is filtered out of the file results.
pub async fn list_dir(bucket: &Bucket, prefix: &str) -> AppResult<Vec<ObjectEntry>> {
    let results = bucket
        .list(prefix.to_string(), Some("/".to_string()))
        .await
        .map_err(|e| AppError::bad_request(format!("S3 list failed: {e}")))?;

    let mut entries = Vec::new();
    for page in results {
        // Sub-folders arrive as common prefixes (e.g. "<prefix>notes/").
        for cp in page.common_prefixes.unwrap_or_default() {
            if let Some(name) = cp.prefix.strip_prefix(prefix) {
                let name = name.trim_end_matches('/');
                if !name.is_empty() {
                    entries.push(ObjectEntry {
                        name: name.to_string(),
                        is_dir: true,
                        size: 0,
                    });
                }
            }
        }
        // Files arrive as contents directly under the prefix.
        for obj in page.contents {
            if let Some(name) = obj.key.strip_prefix(prefix) {
                if name.is_empty() || name == KEEP_MARKER || name.contains('/') {
                    continue; // the prefix itself, a folder marker, or a deeper key
                }
                entries.push(ObjectEntry {
                    name: name.to_string(),
                    is_dir: false,
                    size: obj.size as i64,
                });
            }
        }
    }
    entries.sort_by(|a, b| (b.is_dir, &a.name).cmp(&(a.is_dir, &b.name)));
    Ok(entries)
}

/// Whether a folder `prefix` has any objects under it (used to distinguish an
/// existing-but-empty directory read from a genuine 404).
pub async fn prefix_exists(bucket: &Bucket, prefix: &str) -> AppResult<bool> {
    let results = bucket
        .list(prefix.to_string(), Some("/".to_string()))
        .await
        .map_err(|e| AppError::bad_request(format!("S3 list failed: {e}")))?;
    Ok(results.iter().any(|p| {
        !p.contents.is_empty() || p.common_prefixes.as_ref().is_some_and(|c| !c.is_empty())
    }))
}

/// Delete every object under `prefix` (recursive, no delimiter). Used to drain a
/// folder or an entire docuspace before removing its record.
pub async fn drain_prefix(bucket: &Bucket, prefix: &str) -> AppResult<()> {
    let results = bucket
        .list(prefix.to_string(), None)
        .await
        .map_err(|e| AppError::bad_request(format!("S3 list failed: {e}")))?;
    for page in results {
        for obj in page.contents {
            delete_object(bucket, &obj.key).await?;
        }
    }
    Ok(())
}

/// Zero-byte marker object that materializes an otherwise-empty folder so it shows
/// up in listings and survives until a file is added.
pub const KEEP_MARKER: &str = ".keep";
