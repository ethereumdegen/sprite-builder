use std::time::Duration;

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::github;
use crate::models::{Build, Project, ProjectEnvVar};
use crate::AppState;

/// Max wall-clock for the on-demand `docker logs` exec against the live sprite.
const RUNTIME_LOGS_TIMEOUT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------
// repos
// ---------------------------------------------------------------------------

/// List the GitHub repositories the authenticated user can pick from.
pub async fn list_repos(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
) -> AppResult<Json<Vec<github::GithubRepo>>> {
    let repos = github::list_repos(&app.http, &user.github_token).await?;
    Ok(Json(repos))
}

// ---------------------------------------------------------------------------
// projects
// ---------------------------------------------------------------------------

pub async fn list_projects(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
) -> AppResult<Json<Vec<Project>>> {
    let projects = sqlx::query_as::<_, Project>(
        r#"SELECT id, user_id, name, repo_full_name, repo_id, default_branch,
                  dockerfile_path, container_port, created_at
           FROM projects WHERE user_id = $1 ORDER BY created_at DESC"#,
    )
    .bind(user.id)
    .fetch_all(&app.db)
    .await?;
    Ok(Json(projects))
}

#[derive(Deserialize)]
pub struct CreateProjectBody {
    pub name: String,
    pub repo_full_name: String,
    pub repo_id: Option<i64>,
    pub default_branch: Option<String>,
    pub dockerfile_path: Option<String>,
    pub container_port: Option<i32>,
}

pub async fn create_project(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateProjectBody>,
) -> AppResult<Json<Project>> {
    if body.name.trim().is_empty() || body.repo_full_name.trim().is_empty() {
        return Err(AppError::bad_request("name and repo_full_name are required"));
    }
    let name = body.name.trim().to_string();
    let repo_full_name = body.repo_full_name.trim().to_string();
    let default_branch = body.default_branch.unwrap_or_else(|| "main".to_string());
    let dockerfile_path = body.dockerfile_path.unwrap_or_else(|| "Dockerfile".to_string());
    let container_port = body.container_port.unwrap_or(8080);
    let project = sqlx::query_as::<_, Project>(
        r#"INSERT INTO projects
             (user_id, name, repo_full_name, repo_id, default_branch, dockerfile_path, container_port)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id, user_id, name, repo_full_name, repo_id, default_branch,
                     dockerfile_path, container_port, created_at"#,
    )
    .bind(user.id)
    .bind(name)
    .bind(repo_full_name)
    .bind(body.repo_id)
    .bind(default_branch)
    .bind(dockerfile_path)
    .bind(container_port)
    .fetch_one(&app.db)
    .await?;
    Ok(Json(project))
}

pub(crate) async fn load_owned_project(
    app: &AppState,
    user_id: Uuid,
    id: Uuid,
) -> AppResult<Project> {
    let project = sqlx::query_as::<_, Project>(
        r#"SELECT id, user_id, name, repo_full_name, repo_id, default_branch,
                  dockerfile_path, container_port, created_at
           FROM projects WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(&app.db)
    .await?
    .ok_or(AppError::NotFound)?;
    if project.user_id != user_id {
        return Err(AppError::Forbidden);
    }
    Ok(project)
}

pub async fn get_project(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Project>> {
    Ok(Json(load_owned_project(&app, user.id, id).await?))
}

// ---------------------------------------------------------------------------
// builds
// ---------------------------------------------------------------------------

pub async fn list_builds(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<Build>>> {
    load_owned_project(&app, user.id, id).await?;
    let builds = sqlx::query_as::<_, Build>(
        r#"SELECT id, project_id, commit_sha, status, sprite_name, url, logs, error,
                  metadata, created_at, updated_at, started_at, finished_at
           FROM builds WHERE project_id = $1 ORDER BY created_at DESC"#,
    )
    .bind(id)
    .fetch_all(&app.db)
    .await?;
    Ok(Json(builds))
}

#[derive(Deserialize, Default)]
pub struct CreateBuildBody {
    /// Optional explicit commit. Defaults to the HEAD of the project's branch.
    #[serde(default)]
    pub commit_sha: Option<String>,
}

/// Trigger a new build. Creates a `queued` record that the worker picks up.
pub async fn create_build(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    body: Option<Json<CreateBuildBody>>,
) -> AppResult<Json<Build>> {
    let project = load_owned_project(&app, user.id, id).await?;
    let body = body.map(|Json(b)| b).unwrap_or_default();

    let commit_sha = match body.commit_sha {
        Some(sha) if !sha.trim().is_empty() => sha.trim().to_string(),
        _ => github::latest_commit_sha(
            &app.http,
            &user.github_token,
            &project.repo_full_name,
            &project.default_branch,
        )
        .await
        .map_err(|e| AppError::bad_request(format!("could not resolve HEAD commit: {e}")))?,
    };

    let build = sqlx::query_as::<_, Build>(
        r#"INSERT INTO builds (project_id, commit_sha, status)
           VALUES ($1, $2, 'queued')
           RETURNING id, project_id, commit_sha, status, sprite_name, url, logs, error,
                     metadata, created_at, updated_at, started_at, finished_at"#,
    )
    .bind(project.id)
    .bind(commit_sha)
    .fetch_one(&app.db)
    .await?;

    Ok(Json(build))
}

/// Load a build and enforce ownership via its parent project. Shared by
/// `get_build` and the runtime-logs handler.
async fn load_owned_build(app: &AppState, user_id: Uuid, id: Uuid) -> AppResult<Build> {
    let build = sqlx::query_as::<_, Build>(
        r#"SELECT id, project_id, commit_sha, status, sprite_name, url, logs, error,
                  metadata, created_at, updated_at, started_at, finished_at
           FROM builds WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(&app.db)
    .await?
    .ok_or(AppError::NotFound)?;
    load_owned_project(app, user_id, build.project_id).await?;
    Ok(build)
}

pub async fn get_build(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Build>> {
    Ok(Json(load_owned_build(&app, user.id, id).await?))
}

// ---------------------------------------------------------------------------
// environment variables
// ---------------------------------------------------------------------------

/// Valid env var name: starts with a letter or underscore, then letters/digits/
/// underscores. Mirrors the client-side check.
fn valid_env_key(k: &str) -> bool {
    let mut chars = k.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub async fn list_env_vars(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ProjectEnvVar>>> {
    load_owned_project(&app, user.id, id).await?;
    let vars = sqlx::query_as::<_, ProjectEnvVar>(
        r#"SELECT id, project_id, key, value, created_at, updated_at
           FROM project_env_vars WHERE project_id = $1 ORDER BY key"#,
    )
    .bind(id)
    .fetch_all(&app.db)
    .await?;
    Ok(Json(vars))
}

#[derive(Deserialize)]
pub struct UpsertEnvVarBody {
    pub key: String,
    pub value: String,
}

pub async fn upsert_env_var(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpsertEnvVarBody>,
) -> AppResult<Json<ProjectEnvVar>> {
    load_owned_project(&app, user.id, id).await?;
    let key = body.key.trim().to_string();
    if !valid_env_key(&key) {
        return Err(AppError::bad_request(
            "invalid env var name (use letters, digits, underscores; must not start with a digit)",
        ));
    }
    let var = sqlx::query_as::<_, ProjectEnvVar>(
        r#"INSERT INTO project_env_vars (project_id, key, value)
           VALUES ($1, $2, $3)
           ON CONFLICT (project_id, key)
           DO UPDATE SET value = EXCLUDED.value, updated_at = now()
           RETURNING id, project_id, key, value, created_at, updated_at"#,
    )
    .bind(id)
    .bind(key)
    .bind(body.value)
    .fetch_one(&app.db)
    .await?;
    Ok(Json(var))
}

pub async fn delete_env_var(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path((id, key)): Path<(Uuid, String)>,
) -> AppResult<Json<serde_json::Value>> {
    load_owned_project(&app, user.id, id).await?;
    sqlx::query("DELETE FROM project_env_vars WHERE project_id = $1 AND key = $2")
        .bind(id)
        .bind(key)
        .execute(&app.db)
        .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// runtime logs (on-demand `docker logs` from the live sprite)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct RuntimeLogs {
    pub available: bool,
    pub logs: String,
    pub message: Option<String>,
}

fn runtime_unavailable(message: impl Into<String>) -> Json<RuntimeLogs> {
    Json(RuntimeLogs {
        available: false,
        logs: String::new(),
        message: Some(message.into()),
    })
}

/// Fetch the running container's logs on demand by exec'ing `docker logs` on the
/// build's sprite. No storage — always fresh, and only works while the sprite is
/// alive (which is exactly the "current deployment" model). Env var values are
/// scrubbed before returning (ADR-0013).
pub async fn runtime_logs(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<RuntimeLogs>> {
    let build = load_owned_build(&app, user.id, id).await?;
    let sprite = match &build.sprite_name {
        Some(s) => s.clone(),
        None => return Ok(runtime_unavailable("no deployment yet")),
    };

    // The container is named after the sprite (see worker `build_script`).
    let cmd = format!("sudo docker logs --tail 2000 {sprite} 2>&1");
    let output = match tokio::time::timeout(RUNTIME_LOGS_TIMEOUT, app.sprites.exec(&sprite, &cmd))
        .await
    {
        Ok(Ok(res)) => res.output,
        Ok(Err(e)) => return Ok(runtime_unavailable(format!("could not reach deployment: {e}"))),
        Err(_) => return Ok(runtime_unavailable("timed out fetching runtime logs")),
    };

    // Scrub env var values so secrets the app echoes don't leak into the panel.
    let secrets: Vec<(String,)> =
        sqlx::query_as("SELECT value FROM project_env_vars WHERE project_id = $1")
            .bind(build.project_id)
            .fetch_all(&app.db)
            .await
            .unwrap_or_default();
    let mut logs = output;
    for (value,) in &secrets {
        if !value.is_empty() {
            logs = logs.replace(value.as_str(), "***");
        }
    }

    Ok(Json(RuntimeLogs {
        available: true,
        logs,
        message: None,
    }))
}

// ---------------------------------------------------------------------------
// URL visibility (public vs org-only) for a build's deployment
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct UrlVisibility {
    /// Whether we could read/affect the live sprite at all.
    pub available: bool,
    /// true = anyone with the link; false = org members only ("sprite" auth).
    pub public: bool,
    pub url: Option<String>,
    pub message: Option<String>,
}

pub async fn get_url_visibility(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<UrlVisibility>> {
    let build = load_owned_build(&app, user.id, id).await?;
    let sprite = match &build.sprite_name {
        Some(s) => s.clone(),
        None => {
            return Ok(Json(UrlVisibility {
                available: false,
                public: false,
                url: build.url.clone(),
                message: Some("no deployment yet".into()),
            }))
        }
    };
    match tokio::time::timeout(RUNTIME_LOGS_TIMEOUT, app.sprites.url_auth(&sprite)).await {
        Ok(Ok(auth)) => Ok(Json(UrlVisibility {
            available: true,
            public: auth.as_deref() == Some("public"),
            url: build.url.clone(),
            message: None,
        })),
        _ => Ok(Json(UrlVisibility {
            available: false,
            public: false,
            url: build.url.clone(),
            message: Some("could not reach deployment".into()),
        })),
    }
}

#[derive(Deserialize)]
pub struct SetVisibilityBody {
    pub public: bool,
}

pub async fn set_url_visibility(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<SetVisibilityBody>,
) -> AppResult<Json<UrlVisibility>> {
    let build = load_owned_build(&app, user.id, id).await?;
    let sprite = build
        .sprite_name
        .clone()
        .ok_or_else(|| AppError::bad_request("this build has no deployment to change"))?;
    tokio::time::timeout(RUNTIME_LOGS_TIMEOUT, app.sprites.set_url_auth(&sprite, body.public))
        .await
        .map_err(|_| AppError::bad_request("timed out updating URL visibility"))?
        .map_err(|e| AppError::bad_request(format!("could not update URL visibility: {e}")))?;
    Ok(Json(UrlVisibility {
        available: true,
        public: body.public,
        url: build.url.clone(),
        message: None,
    }))
}
