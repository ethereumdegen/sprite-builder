//! Docuspaces — a project's S3-backed file store (usually markdown), the third
//! sub-application alongside builds and codespaces (ADR 0001: thin handlers in a
//! domain module).
//!
//! Unlike a codespace there is **no sprite and no worker**: a docuspace is just a
//! record plus a key prefix (`docuspaces/<id>/...`) in the configured S3 bucket.
//! Files are stored as plain objects and folders are *implicit* — a folder exists
//! when something lives under it, with an optional zero-byte `.keep` marker
//! (`storage::KEEP_MARKER`) to materialize an otherwise-empty one. Reads and writes
//! proxy bytes through the backend (fully auth-gated) rather than presigning.
//!
//! Caller-supplied paths are validated by the shared `util::validate_rel_path`
//! (relative only, no `..`); the S3 key is then `docuspaces/<id>/<rel>`, so a path
//! can never escape its docuspace's prefix.

use axum::extract::{Path, Query, State};
use axum::Json;
use base64::Engine;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::models::Docuspace;
use crate::projects::load_owned_project;
use crate::storage::{self, ObjectEntry};
use crate::util::{random_name, validate_rel_path};
use crate::AppState;

/// Cap on a single file's bytes we'll accept on write (5 MiB). Docuspaces are for
/// documents, not large media; this keeps a proxied write bounded.
const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;

// ---------------------------------------------------------------------------
// loading / ownership / keys
// ---------------------------------------------------------------------------

const DOCUSPACE_COLS: &str =
    "id, project_id, name, metadata, created_at, updated_at";

/// Load a docuspace and enforce ownership via its parent project (ADR 0003).
async fn load_owned_docuspace(app: &AppState, user_id: Uuid, id: Uuid) -> AppResult<Docuspace> {
    let ds = sqlx::query_as::<_, Docuspace>(&format!(
        "SELECT {DOCUSPACE_COLS} FROM docuspaces WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&app.db)
    .await?
    .ok_or(AppError::NotFound)?;
    load_owned_project(app, user_id, ds.project_id).await?;
    Ok(ds)
}

/// The S3 key prefix all of a docuspace's objects live under (trailing slash).
fn prefix(id: Uuid) -> String {
    format!("docuspaces/{id}/")
}

/// Bump `updated_at` after a mutating file op so listings reflect recent activity.
async fn touch(app: &AppState, id: Uuid) {
    let _ = sqlx::query("UPDATE docuspaces SET updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(&app.db)
        .await;
}

// ---------------------------------------------------------------------------
// lifecycle: list / create / get / rename / delete
// ---------------------------------------------------------------------------

pub async fn list_docuspaces(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<Vec<Docuspace>>> {
    load_owned_project(&app, user.id, project_id).await?;
    let list = sqlx::query_as::<_, Docuspace>(&format!(
        "SELECT {DOCUSPACE_COLS} FROM docuspaces WHERE project_id = $1 ORDER BY created_at DESC"
    ))
    .bind(project_id)
    .fetch_all(&app.db)
    .await?;
    Ok(Json(list))
}

#[derive(Deserialize, Default)]
pub struct CreateDocuspaceBody {
    /// Optional friendly name. Defaults to a random adjective-noun pair.
    #[serde(default)]
    pub name: Option<String>,
}

/// Create a docuspace: just a record. Nothing is written to S3 until the first
/// file is uploaded (folders are implicit), so creation is instant.
pub async fn create_docuspace(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(project_id): Path<Uuid>,
    body: Option<Json<CreateDocuspaceBody>>,
) -> AppResult<Json<Docuspace>> {
    let project = load_owned_project(&app, user.id, project_id).await?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let name = body
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(random_name);

    let ds = sqlx::query_as::<_, Docuspace>(&format!(
        "INSERT INTO docuspaces (project_id, name) VALUES ($1, $2) RETURNING {DOCUSPACE_COLS}"
    ))
    .bind(project.id)
    .bind(name)
    .fetch_one(&app.db)
    .await?;
    Ok(Json(ds))
}

pub async fn get_docuspace(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Docuspace>> {
    Ok(Json(load_owned_docuspace(&app, user.id, id).await?))
}

#[derive(Deserialize)]
pub struct RenameDocuspaceBody {
    pub name: String,
}

/// Rename a docuspace — the friendly label only; the S3 prefix and its objects
/// are keyed by id, so nothing in storage moves.
pub async fn rename_docuspace(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<RenameDocuspaceBody>,
) -> AppResult<Json<Docuspace>> {
    load_owned_docuspace(&app, user.id, id).await?;
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::bad_request("name is required"));
    }
    let ds = sqlx::query_as::<_, Docuspace>(&format!(
        "UPDATE docuspaces SET name = $1, updated_at = now() WHERE id = $2 RETURNING {DOCUSPACE_COLS}"
    ))
    .bind(name)
    .bind(id)
    .fetch_one(&app.db)
    .await?;
    Ok(Json(ds))
}

/// Destroy a docuspace: drain every object under its prefix, then delete the row.
pub async fn delete_docuspace(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let ds = load_owned_docuspace(&app, user.id, id).await?;
    // Best-effort storage cleanup: if S3 isn't configured we still drop the row
    // rather than wedging the docuspace as undeletable.
    if let Ok(bucket) = storage::build_bucket(&app.config) {
        storage::drain_prefix(&bucket, &prefix(ds.id)).await?;
    }
    sqlx::query("DELETE FROM docuspaces WHERE id = $1")
        .bind(id)
        .execute(&app.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// files: read / list / write / delete + folders
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PathQuery {
    #[serde(default)]
    pub path: Option<String>,
}

/// A read of a docuspace path: a directory listing (`entries`) or a file. Text
/// files arrive decoded in `content`; non-UTF-8 files arrive base64 with `binary`.
#[derive(Serialize)]
pub struct ReadResult {
    /// "dir" or "file".
    pub kind: String,
    pub path: String,
    pub entries: Option<Vec<ObjectEntry>>,
    pub content: Option<String>,
    pub binary: bool,
    pub size: i64,
}

/// Read a file or list a directory. `path` empty/omitted = the docuspace root.
/// A path resolves to a file if an object exists at that exact key; otherwise, if
/// anything lives under `<path>/`, it's treated as a directory.
pub async fn read_path(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<PathQuery>,
) -> AppResult<Json<ReadResult>> {
    let ds = load_owned_docuspace(&app, user.id, id).await?;
    let bucket = storage::build_bucket(&app.config)?;
    let rel = validate_rel_path(q.path.as_deref().unwrap_or(""), true)?;
    let base = prefix(ds.id);

    // Root, or an explicit trailing slash, is always a directory listing.
    if rel.is_empty() {
        let entries = storage::list_dir(&bucket, &base).await?;
        return Ok(Json(dir_result(&rel, entries)));
    }

    let key = format!("{base}{rel}");
    if let Some(bytes) = storage::get_bytes(&bucket, &key).await? {
        return Ok(Json(file_result(&rel, bytes)));
    }

    // Not an object at that key — maybe it's a folder.
    let dir_prefix = format!("{key}/");
    if storage::prefix_exists(&bucket, &dir_prefix).await? {
        let entries = storage::list_dir(&bucket, &dir_prefix).await?;
        return Ok(Json(dir_result(&rel, entries)));
    }

    Err(AppError::NotFound)
}

fn dir_result(rel: &str, entries: Vec<ObjectEntry>) -> ReadResult {
    ReadResult {
        kind: "dir".to_string(),
        path: rel.to_string(),
        entries: Some(entries),
        content: None,
        binary: false,
        size: 0,
    }
}

fn file_result(rel: &str, bytes: Vec<u8>) -> ReadResult {
    let size = bytes.len() as i64;
    match String::from_utf8(bytes) {
        Ok(text) => ReadResult {
            kind: "file".to_string(),
            path: rel.to_string(),
            entries: None,
            content: Some(text),
            binary: false,
            size,
        },
        Err(e) => ReadResult {
            kind: "file".to_string(),
            path: rel.to_string(),
            entries: None,
            content: Some(
                base64::engine::general_purpose::STANDARD.encode(e.as_bytes()),
            ),
            binary: true,
            size,
        },
    }
}

#[derive(Deserialize)]
pub struct WriteFileBody {
    pub path: String,
    pub content: String,
    /// "utf8" (default) or "base64" — use base64 to upload binary files as text.
    #[serde(default)]
    pub encoding: Option<String>,
}

/// Write (create or overwrite) a file. The content type is inferred from the
/// extension so markdown/images render correctly on download.
pub async fn write_file(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<WriteFileBody>,
) -> AppResult<Json<serde_json::Value>> {
    let ds = load_owned_docuspace(&app, user.id, id).await?;
    let bucket = storage::build_bucket(&app.config)?;
    let rel = validate_rel_path(&body.path, false)?;

    let bytes = match body.encoding.as_deref() {
        Some("base64") => base64::engine::general_purpose::STANDARD
            .decode(body.content.trim())
            .map_err(|_| AppError::bad_request("content is not valid base64"))?,
        _ => body.content.into_bytes(),
    };
    if bytes.len() > MAX_FILE_BYTES {
        return Err(AppError::bad_request("file is too large to write (max 5 MiB)"));
    }

    let key = format!("{}{}", prefix(ds.id), rel);
    storage::put_bytes(&bucket, &key, &bytes, content_type_for(&rel)).await?;
    touch(&app, ds.id).await;
    Ok(Json(serde_json::json!({ "ok": true, "path": rel })))
}

/// Delete a file, or a folder and everything under it.
pub async fn delete_path(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<PathQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let ds = load_owned_docuspace(&app, user.id, id).await?;
    let bucket = storage::build_bucket(&app.config)?;
    let rel = validate_rel_path(q.path.as_deref().unwrap_or(""), false)?;
    let key = format!("{}{}", prefix(ds.id), rel);

    // Delete the object itself (if it's a file) and drain any folder under it.
    storage::delete_object(&bucket, &key).await?;
    storage::drain_prefix(&bucket, &format!("{key}/")).await?;
    touch(&app, ds.id).await;
    Ok(Json(serde_json::json!({ "ok": true, "path": rel })))
}

#[derive(Deserialize)]
pub struct CreateFolderBody {
    pub path: String,
}

/// Create an (empty) folder by writing its `.keep` marker. Folders are otherwise
/// implicit, so this only matters for a folder you want visible before it has
/// any files.
pub async fn create_folder(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateFolderBody>,
) -> AppResult<Json<serde_json::Value>> {
    let ds = load_owned_docuspace(&app, user.id, id).await?;
    let bucket = storage::build_bucket(&app.config)?;
    let rel = validate_rel_path(&body.path, false)?;
    let key = format!("{}{}/{}", prefix(ds.id), rel, storage::KEEP_MARKER);
    storage::put_bytes(&bucket, &key, b"", "application/octet-stream").await?;
    touch(&app, ds.id).await;
    Ok(Json(serde_json::json!({ "ok": true, "path": rel })))
}

/// Best-effort content type from a file's extension. Defaults to a generic binary
/// type, which is safe for download.
fn content_type_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "txt" | "text" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "yaml" | "yml" => "application/yaml",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
