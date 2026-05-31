use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::github;
use crate::models::{Build, Project};
use crate::AppState;

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
        "SELECT * FROM projects WHERE user_id = $1 ORDER BY created_at DESC",
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
    let project = sqlx::query_as::<_, Project>(
        r#"INSERT INTO projects
             (user_id, name, repo_full_name, repo_id, default_branch, dockerfile_path, container_port)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING *"#,
    )
    .bind(user.id)
    .bind(body.name.trim())
    .bind(body.repo_full_name.trim())
    .bind(body.repo_id)
    .bind(body.default_branch.unwrap_or_else(|| "main".to_string()))
    .bind(body.dockerfile_path.unwrap_or_else(|| "Dockerfile".to_string()))
    .bind(body.container_port.unwrap_or(8080))
    .fetch_one(&app.db)
    .await?;
    Ok(Json(project))
}

async fn load_owned_project(app: &AppState, user_id: Uuid, id: Uuid) -> AppResult<Project> {
    let project = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1")
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
        "SELECT * FROM builds WHERE project_id = $1 ORDER BY created_at DESC",
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
           VALUES ($1, $2, 'queued') RETURNING *"#,
    )
    .bind(project.id)
    .bind(&commit_sha)
    .fetch_one(&app.db)
    .await?;

    Ok(Json(build))
}

pub async fn get_build(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Build>> {
    let build = sqlx::query_as::<_, Build>("SELECT * FROM builds WHERE id = $1")
        .bind(id)
        .fetch_optional(&app.db)
        .await?
        .ok_or(AppError::NotFound)?;
    // Ownership check via the parent project.
    load_owned_project(&app, user.id, build.project_id).await?;
    Ok(Json(build))
}
