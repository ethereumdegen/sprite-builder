//! Codespaces — an ephemeral coding filesystem (ADR 0001: thin handlers in a
//! domain module).
//!
//! A Codespace is a long-lived sprite holding a git working tree at
//! `/workspace/app`. Lifecycle (create/provision) is asynchronous and handled by
//! the worker; the interactive operations here (files, exec, git) run
//! *synchronously* against the live sprite via the Sprites exec endpoint — the
//! same bounded-exec pattern as `projects::runtime_logs`.
//!
//! All dynamic values that reach the in-sprite shell (paths, file contents, the
//! exec command, commit messages, the git token) are shipped **base64-encoded and
//! decoded inside the sprite**, so nothing the caller sends can break out of its
//! shell variable — the same hardening `worker::build_script` uses. Filesystem
//! ops are additionally jailed under `/workspace/app` (rejected client-side, then
//! re-checked in-sprite with `realpath`). Output is redacted of project secrets
//! before it's returned (ADR 0013).

use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::Json;
use base64::Engine;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::models::Codespace;
use crate::projects::load_owned_project;
use crate::AppState;

/// Where the repo is cloned inside the sprite (see the worker's clone script).
const WORKDIR: &str = "/workspace/app";

/// Max wall-clock for a filesystem op (read/list/write/delete) against the sprite.
const FILE_TIMEOUT: Duration = Duration::from_secs(30);

/// Max wall-clock for an arbitrary `exec` or a git op (push/pull can be slow).
const EXEC_TIMEOUT: Duration = Duration::from_secs(120);

/// Cap on a single file's bytes we'll read back or accept on write (1 MiB). Keeps
/// one synchronous exec sane; larger files are reported `truncated` on read.
const MAX_FILE_BYTES: usize = 1024 * 1024;

// Markers the in-sprite scripts emit so we can frame their output. Chosen to be
// improbable in real file/command output.
const KIND_MARKER: &str = "__CS_KIND__:";
const SIZE_MARKER: &str = "__CS_SIZE__:";
const BIN_MARKER: &str = "__CS_BIN__:";
const TRUNC_MARKER: &str = "__CS_TRUNC__:";
const BODY_MARKER: &str = "__CS_BODY__";
const OK_MARKER: &str = "__CS_OK__";
const ERR_MARKER: &str = "__CS_ERR__:";
const EXIT_MARKER: &str = "__CS_EXIT__:";

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

// ---------------------------------------------------------------------------
// loading / ownership
// ---------------------------------------------------------------------------

/// Load a codespace and enforce ownership via its parent project (ADR 0003).
async fn load_owned_codespace(app: &AppState, user_id: Uuid, id: Uuid) -> AppResult<Codespace> {
    let cs = sqlx::query_as::<_, Codespace>(
        r#"SELECT id, project_id, name, branch, status, sprite_name, url, snapshot_key,
                  logs, error, metadata, created_at, updated_at, started_at, finished_at
           FROM codespaces WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(&app.db)
    .await?
    .ok_or(AppError::NotFound)?;
    load_owned_project(app, user_id, cs.project_id).await?;
    Ok(cs)
}

/// The live sprite backing a `ready` codespace, or a 400 explaining why it isn't
/// reachable (still provisioning, failed, or stopped/hibernated).
fn live_sprite(cs: &Codespace) -> AppResult<String> {
    if cs.status != "ready" {
        return Err(AppError::bad_request(format!(
            "codespace is not ready (status: {})",
            cs.status
        )));
    }
    cs.sprite_name
        .clone()
        .ok_or_else(|| AppError::bad_request("codespace has no live sprite"))
}

/// Project env-var values, redacted from any sprite output we return (ADR 0013).
async fn redaction_secrets(app: &AppState, project_id: Uuid) -> Vec<String> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT value FROM project_env_vars WHERE project_id = $1")
            .bind(project_id)
            .fetch_all(&app.db)
            .await
            .unwrap_or_default();
    rows.into_iter()
        .map(|(v,)| v)
        .filter(|v| !v.is_empty())
        .collect()
}

fn redact(mut s: String, secrets: &[String]) -> String {
    for secret in secrets {
        if !secret.is_empty() {
            s = s.replace(secret.as_str(), "***");
        }
    }
    s
}

/// Run a script on the codespace's sprite with a bound, mapping transport
/// failures to clean HTTP errors (ADR 0010/0012). Returns the raw combined output.
async fn run(app: &AppState, sprite: &str, script: &str, timeout: Duration) -> AppResult<String> {
    match tokio::time::timeout(timeout, app.sprites.exec(sprite, script)).await {
        Ok(Ok(res)) => Ok(res.output),
        Ok(Err(e)) => Err(AppError::bad_request(format!(
            "could not reach codespace: {e}"
        ))),
        Err(_) => Err(AppError::bad_request("timed out talking to the codespace")),
    }
}

// ---------------------------------------------------------------------------
// path jailing
// ---------------------------------------------------------------------------

/// Client-side validation of a caller-supplied relative path. The in-sprite
/// `realpath` guard is the real backstop (it also defeats symlink escapes); this
/// rejects the obvious cases early and keeps a tidy 400.
fn validate_rel_path(path: &str, allow_root: bool) -> AppResult<String> {
    let p = path.trim().trim_start_matches("./").to_string();
    if p.is_empty() || p == "." {
        if allow_root {
            return Ok(String::new());
        }
        return Err(AppError::bad_request("a file path is required"));
    }
    if p.starts_with('/') || p.starts_with('~') {
        return Err(AppError::bad_request("path must be relative to the workspace"));
    }
    if p.contains('\0') {
        return Err(AppError::bad_request("invalid path"));
    }
    if p.split('/').any(|seg| seg == "..") {
        return Err(AppError::bad_request("path may not traverse outside the workspace"));
    }
    Ok(p)
}

/// Bash snippet that resolves `$P` (already decoded) under WORKDIR into `$R` and
/// aborts with an ERR marker if it escapes. Shared by every filesystem op.
fn jail_prelude() -> String {
    format!(
        r#"set -uo pipefail
cd {WORKDIR} 2>/dev/null || {{ echo "{ERR_MARKER}workspace is missing"; exit 4; }}
T="{WORKDIR}/$P"
R=$(realpath -m -- "$T")
case "$R" in
  {WORKDIR}|{WORKDIR}/*) ;;
  *) echo "{ERR_MARKER}path escapes the workspace"; exit 3 ;;
esac"#
    )
}

/// Parse a leading `__CS_ERR__:<msg>` from sprite output into a 400, if present.
fn check_err(output: &str) -> AppResult<()> {
    if let Some(line) = output.lines().find(|l| l.starts_with(ERR_MARKER)) {
        return Err(AppError::bad_request(
            line[ERR_MARKER.len()..].trim().to_string(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// lifecycle: list / create / get / delete
// ---------------------------------------------------------------------------

pub async fn list_codespaces(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<Vec<Codespace>>> {
    load_owned_project(&app, user.id, project_id).await?;
    let list = sqlx::query_as::<_, Codespace>(
        r#"SELECT id, project_id, name, branch, status, sprite_name, url, snapshot_key,
                  logs, error, metadata, created_at, updated_at, started_at, finished_at
           FROM codespaces WHERE project_id = $1 ORDER BY created_at DESC"#,
    )
    .bind(project_id)
    .fetch_all(&app.db)
    .await?;
    Ok(Json(list))
}

#[derive(Deserialize, Default)]
pub struct CreateCodespaceBody {
    /// Optional friendly name. Defaults to a random adjective-noun pair.
    #[serde(default)]
    pub name: Option<String>,
}

/// A random adjective-noun label (e.g. `selfish-change`) from the `names` crate —
/// the same source as the sprite subdomain — used as a codespace's default name.
/// Created and dropped in one expression so the non-Send generator never crosses
/// an await.
fn random_name() -> String {
    names::Generator::default()
        .next()
        .unwrap_or_else(|| "codespace".to_string())
}

/// Create a codespace: insert a `queued` row the worker provisions. The branch is
/// the project's default branch in v1 (the column exists for future selection).
pub async fn create_codespace(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(project_id): Path<Uuid>,
    body: Option<Json<CreateCodespaceBody>>,
) -> AppResult<Json<Codespace>> {
    let project = load_owned_project(&app, user.id, project_id).await?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let branch = project.default_branch.clone();
    let name = body
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(random_name);

    let cs = sqlx::query_as::<_, Codespace>(
        r#"INSERT INTO codespaces (project_id, name, branch, status)
           VALUES ($1, $2, $3, 'queued')
           RETURNING id, project_id, name, branch, status, sprite_name, url, snapshot_key,
                     logs, error, metadata, created_at, updated_at, started_at, finished_at"#,
    )
    .bind(project.id)
    .bind(name)
    .bind(branch)
    .fetch_one(&app.db)
    .await?;
    Ok(Json(cs))
}

pub async fn get_codespace(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Codespace>> {
    Ok(Json(load_owned_codespace(&app, user.id, id).await?))
}

#[derive(Deserialize)]
pub struct RenameCodespaceBody {
    pub name: String,
}

/// Rename a codespace — the friendly label only; the sprite, branch, and clone
/// are untouched.
pub async fn rename_codespace(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<RenameCodespaceBody>,
) -> AppResult<Json<Codespace>> {
    load_owned_codespace(&app, user.id, id).await?;
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::bad_request("name is required"));
    }
    let cs = sqlx::query_as::<_, Codespace>(
        r#"UPDATE codespaces SET name = $1, updated_at = now() WHERE id = $2
           RETURNING id, project_id, name, branch, status, sprite_name, url, snapshot_key,
                     logs, error, metadata, created_at, updated_at, started_at, finished_at"#,
    )
    .bind(name)
    .bind(id)
    .fetch_one(&app.db)
    .await?;
    Ok(Json(cs))
}

/// Destroy a codespace: best-effort delete of the live sprite, then the row.
pub async fn delete_codespace(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let cs = load_owned_codespace(&app, user.id, id).await?;
    if let Some(sprite) = &cs.sprite_name {
        let _ = app.sprites.delete_sprite(sprite).await;
    }
    sqlx::query("DELETE FROM codespaces WHERE id = $1")
        .bind(id)
        .execute(&app.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// files: read / list / write / delete
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PathQuery {
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
}

#[derive(Serialize)]
pub struct ReadResult {
    /// "dir" or "file".
    pub kind: String,
    pub path: String,
    /// Directory entries (when `kind == "dir"`).
    pub entries: Option<Vec<FileEntry>>,
    /// Text content (when `kind == "file"` and `binary` is false); base64 of the
    /// (possibly truncated) bytes when `binary` is true.
    pub content: Option<String>,
    pub binary: bool,
    pub truncated: bool,
    pub size: i64,
}

/// Read a file or list a directory under the workspace. `path` empty/omitted =
/// the workspace root.
pub async fn read_path(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<PathQuery>,
) -> AppResult<Json<ReadResult>> {
    let cs = load_owned_codespace(&app, user.id, id).await?;
    let sprite = live_sprite(&cs)?;
    let rel = validate_rel_path(q.path.as_deref().unwrap_or(""), true)?;

    let script = format!(
        r#"P=$(printf %s '{path_b64}' | base64 -d)
{prelude}
if [ -d "$R" ]; then
  echo "{KIND_MARKER}dir"
  ls -A1p -- "$R"
elif [ -f "$R" ]; then
  SZ=$(stat -c %s -- "$R" 2>/dev/null || echo 0)
  if head -c 8192 -- "$R" | grep -qP '\x00' 2>/dev/null; then BIN=1; else BIN=0; fi
  if [ "$SZ" -gt {max} ]; then TRUNC=1; else TRUNC=0; fi
  echo "{KIND_MARKER}file"
  echo "{SIZE_MARKER}$SZ"
  echo "{BIN_MARKER}$BIN"
  echo "{TRUNC_MARKER}$TRUNC"
  echo "{BODY_MARKER}"
  head -c {max} -- "$R" | base64
else
  echo "{ERR_MARKER}no such file or directory"; exit 5
fi"#,
        path_b64 = b64(&rel),
        prelude = jail_prelude(),
        max = MAX_FILE_BYTES,
    );

    let out = run(&app, &sprite, &script, FILE_TIMEOUT).await?;
    check_err(&out)?;
    parse_read(&out, &rel).map(Json)
}

/// Parse the framed read/list output into a [`ReadResult`].
fn parse_read(out: &str, rel: &str) -> AppResult<ReadResult> {
    let kind = marker_value(out, KIND_MARKER).unwrap_or_default();
    match kind.as_str() {
        "dir" => {
            // Everything after the KIND line is one entry per line; `ls -A1p`
            // suffixes directories with '/'.
            let entries = out
                .lines()
                .skip_while(|l| !l.starts_with(KIND_MARKER))
                .skip(1)
                .filter(|l| !l.is_empty())
                .map(|l| {
                    let is_dir = l.ends_with('/');
                    FileEntry {
                        name: l.trim_end_matches('/').to_string(),
                        is_dir,
                    }
                })
                .collect();
            Ok(ReadResult {
                kind,
                path: rel.to_string(),
                entries: Some(entries),
                content: None,
                binary: false,
                truncated: false,
                size: 0,
            })
        }
        "file" => {
            let size = marker_value(out, SIZE_MARKER)
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let binary = marker_value(out, BIN_MARKER).as_deref() == Some("1");
            let truncated = marker_value(out, TRUNC_MARKER).as_deref() == Some("1");
            // Body is the base64 blob after the BODY marker line.
            let b64body: String = out
                .lines()
                .skip_while(|l| *l != BODY_MARKER)
                .skip(1)
                .collect::<Vec<_>>()
                .join("");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64body.trim())
                .unwrap_or_default();
            let content = if binary {
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            };
            Ok(ReadResult {
                kind,
                path: rel.to_string(),
                entries: None,
                content: Some(content),
                binary,
                truncated,
                size,
            })
        }
        _ => Err(AppError::bad_request("unexpected codespace response")),
    }
}

/// First line value for `marker` (the text following the marker prefix).
fn marker_value(out: &str, marker: &str) -> Option<String> {
    out.lines()
        .find(|l| l.starts_with(marker))
        .map(|l| l[marker.len()..].trim().to_string())
}

#[derive(Deserialize)]
pub struct WriteFileBody {
    pub path: String,
    pub content: String,
}

pub async fn write_file(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<WriteFileBody>,
) -> AppResult<Json<serde_json::Value>> {
    let cs = load_owned_codespace(&app, user.id, id).await?;
    let sprite = live_sprite(&cs)?;
    let rel = validate_rel_path(&body.path, false)?;
    if body.content.len() > MAX_FILE_BYTES {
        return Err(AppError::bad_request("file is too large to write (max 1 MiB)"));
    }

    let script = format!(
        r#"P=$(printf %s '{path_b64}' | base64 -d)
{prelude}
if [ "$R" = "{WORKDIR}" ]; then echo "{ERR_MARKER}refusing to write the workspace root"; exit 6; fi
mkdir -p -- "$(dirname -- "$R")"
printf %s '{content_b64}' | base64 -d > "$R"
echo "{OK_MARKER}""#,
        path_b64 = b64(&rel),
        content_b64 = b64(&body.content),
        prelude = jail_prelude(),
    );
    let out = run(&app, &sprite, &script, FILE_TIMEOUT).await?;
    check_err(&out)?;
    if !out.contains(OK_MARKER) {
        return Err(AppError::bad_request("write failed"));
    }
    Ok(Json(serde_json::json!({ "ok": true, "path": rel })))
}

pub async fn delete_path(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<PathQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let cs = load_owned_codespace(&app, user.id, id).await?;
    let sprite = live_sprite(&cs)?;
    let rel = validate_rel_path(q.path.as_deref().unwrap_or(""), false)?;

    let script = format!(
        r#"P=$(printf %s '{path_b64}' | base64 -d)
{prelude}
if [ "$R" = "{WORKDIR}" ]; then echo "{ERR_MARKER}refusing to delete the workspace root"; exit 6; fi
rm -rf -- "$R"
echo "{OK_MARKER}""#,
        path_b64 = b64(&rel),
        prelude = jail_prelude(),
    );
    let out = run(&app, &sprite, &script, FILE_TIMEOUT).await?;
    check_err(&out)?;
    Ok(Json(serde_json::json!({ "ok": true, "path": rel })))
}

// ---------------------------------------------------------------------------
// exec: arbitrary bash ("as if on the local machine")
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ExecBody {
    pub cmd: String,
}

#[derive(Serialize)]
pub struct ExecResponse {
    pub output: String,
    pub exit_code: i32,
}

/// Run an arbitrary command in the workspace and return its merged output + exit
/// code. The command is decoded inside the sprite (injection-safe) and runs from
/// the workspace dir.
pub async fn exec(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<ExecBody>,
) -> AppResult<Json<ExecResponse>> {
    let cs = load_owned_codespace(&app, user.id, id).await?;
    let sprite = live_sprite(&cs)?;
    if body.cmd.trim().is_empty() {
        return Err(AppError::bad_request("cmd is required"));
    }

    let script = format!(
        r#"cd {WORKDIR} 2>/dev/null || true
CMD=$(printf %s '{cmd_b64}' | base64 -d)
bash -c "$CMD" 2>&1
echo "{EXIT_MARKER}$?""#,
        cmd_b64 = b64(&body.cmd),
    );
    let out = run(&app, &sprite, &script, EXEC_TIMEOUT).await?;
    let (output, exit_code) = split_exit(&out);
    let secrets = redaction_secrets(&app, cs.project_id).await;
    Ok(Json(ExecResponse {
        output: redact(output, &secrets),
        exit_code,
    }))
}

/// Split a trailing `__CS_EXIT__:<n>` off command output. Defaults the code to -1
/// if the marker is missing (the exec channel dropped before it printed).
fn split_exit(out: &str) -> (String, i32) {
    if let Some(idx) = out.rfind(EXIT_MARKER) {
        let code = out[idx + EXIT_MARKER.len()..]
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .parse::<i32>()
            .unwrap_or(-1);
        (out[..idx].trim_end().to_string(), code)
    } else {
        (out.trim_end().to_string(), -1)
    }
}

// ---------------------------------------------------------------------------
// git: status / diff / commit / push / pull
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct GitBody {
    /// One of: status | diff | commit | push | pull.
    pub op: String,
    /// Commit message (required for `commit`).
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct GitResponse {
    pub op: String,
    pub output: String,
    pub exit_code: i32,
    pub ok: bool,
}

/// Run a git operation in the workspace. `push`/`pull` authenticate to GitHub via
/// a credential helper that reads the token from an env var (never the remote
/// URL), and `commit` takes a base64-shipped message. The owner's token is
/// redacted from all output (ADR 0013).
pub async fn git(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<GitBody>,
) -> AppResult<Json<GitResponse>> {
    let cs = load_owned_codespace(&app, user.id, id).await?;
    let sprite = live_sprite(&cs)?;
    let branch = b64(&cs.branch);

    let git_body = match body.op.as_str() {
        "status" => "git -C app status --porcelain=v1 -b".to_string(),
        "diff" => "git -C app diff HEAD".to_string(),
        "commit" => {
            let msg = body
                .message
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .ok_or_else(|| AppError::bad_request("a commit message is required"))?;
            format!(
                r#"MSG=$(printf %s '{msg_b64}' | base64 -d)
git -C app add -A
git -C app commit -m "$MSG""#,
                msg_b64 = b64(msg),
            )
        }
        "push" | "pull" => {
            // The token is needed only here; fetch it now and redact it after.
            let token = owner_token(&app, cs.project_id).await?;
            let helper = r#"credential.helper='!f() { echo username=x-access-token; echo "password=$CS_GH_TOKEN"; }; f'"#;
            let action = if body.op == "push" {
                "push origin \"$BRANCH\""
            } else {
                "pull --ff-only origin \"$BRANCH\""
            };
            format!(
                r#"export CS_GH_TOKEN='{token}'
BRANCH=$(printf %s '{branch}' | base64 -d)
git -C app -c {helper} {action}"#,
            )
        }
        other => {
            return Err(AppError::bad_request(format!("unknown git op: {other}")));
        }
    };

    let script = format!(
        r#"cd {WORKDIR}/.. 2>/dev/null || true
{git_body}
echo "{EXIT_MARKER}$?""#,
    );
    let out = run(&app, &sprite, &script, EXEC_TIMEOUT).await?;
    let (output, exit_code) = split_exit(&out);

    // Redact env values + the token (defense in depth — push/pull never echo it,
    // but a failed clone-style error could).
    let mut secrets = redaction_secrets(&app, cs.project_id).await;
    if let Ok(tok) = owner_token(&app, cs.project_id).await {
        secrets.push(tok);
    }
    Ok(Json(GitResponse {
        op: body.op,
        output: redact(output, &secrets),
        exit_code,
        ok: exit_code == 0,
    }))
}

/// The GitHub token of the project's owner (used for push/pull credentials).
async fn owner_token(app: &AppState, project_id: Uuid) -> AppResult<String> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"SELECT u.github_token
           FROM users u JOIN projects p ON p.user_id = u.id
           WHERE p.id = $1"#,
    )
    .bind(project_id)
    .fetch_optional(&app.db)
    .await?;
    row.map(|(t,)| t)
        .ok_or_else(|| AppError::bad_request("project owner not found"))
}
