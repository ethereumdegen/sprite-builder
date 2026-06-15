//! Admin dashboard endpoints: cross-tenant build/diagnostic visibility and user
//! role management. Every handler is gated by the [`AdminUser`] extractor, which
//! requires the admin capability (ADR 0004 + 0016) — there are no inline role
//! checks here.

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AdminUser;
use crate::error::{AppError, AppResult};
use crate::AppState;

// ---------------------------------------------------------------------------
// stats
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
pub struct AdminStats {
    pub users: i64,
    pub projects: i64,
    pub builds_total: i64,
    pub builds_queued: i64,
    pub builds_running: i64,
    pub builds_succeeded: i64,
    pub builds_failed: i64,
}

/// App-wide counts for the dashboard header.
pub async fn stats(State(app): State<AppState>, _admin: AdminUser) -> AppResult<Json<AdminStats>> {
    let stats = sqlx::query_as::<_, AdminStats>(
        r#"SELECT
            (SELECT count(*) FROM users)                              AS users,
            (SELECT count(*) FROM projects)                          AS projects,
            (SELECT count(*) FROM builds)                            AS builds_total,
            (SELECT count(*) FROM builds WHERE status = 'queued')    AS builds_queued,
            (SELECT count(*) FROM builds WHERE status = 'running')   AS builds_running,
            (SELECT count(*) FROM builds WHERE status = 'succeeded') AS builds_succeeded,
            (SELECT count(*) FROM builds WHERE status = 'failed')    AS builds_failed
        "#,
    )
    .fetch_one(&app.db)
    .await?;

    Ok(Json(stats))
}

// ---------------------------------------------------------------------------
// builds (cross-tenant)
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
pub struct AdminBuild {
    pub id: Uuid,
    pub status: String,
    pub commit_sha: String,
    pub sprite_name: Option<String>,
    pub url: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub project_id: Uuid,
    pub project_name: String,
    pub repo_full_name: String,
    pub owner_login: String,
}

#[derive(Deserialize)]
pub struct BuildsQuery {
    /// Optional status filter (queued | running | succeeded | failed).
    pub status: Option<String>,
    /// Max rows to return (default 200, capped at 1000).
    pub limit: Option<i64>,
}

/// Every build across the whole app, newest first, with owner/project context.
pub async fn builds(
    State(app): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<BuildsQuery>,
) -> AppResult<Json<Vec<AdminBuild>>> {
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let status = q.status.filter(|s| !s.trim().is_empty());

    let rows = sqlx::query_as::<_, AdminBuild>(
        r#"SELECT
              b.id            AS id,
              b.status        AS status,
              b.commit_sha    AS commit_sha,
              b.sprite_name   AS sprite_name,
              b.url           AS url,
              b.error         AS error,
              b.created_at    AS created_at,
              b.started_at    AS started_at,
              b.finished_at   AS finished_at,
              b.project_id    AS project_id,
              p.name          AS project_name,
              p.repo_full_name AS repo_full_name,
              u.github_login  AS owner_login
           FROM builds b
           JOIN projects p ON p.id = b.project_id
           JOIN users u    ON u.id = p.user_id
           WHERE ($1::text IS NULL OR b.status = $1)
           ORDER BY b.created_at DESC
           LIMIT $2"#,
    )
    .bind(status)
    .bind(limit)
    .fetch_all(&app.db)
    .await?;

    Ok(Json(rows))
}

// ---------------------------------------------------------------------------
// users + role management
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
pub struct AdminUserRow {
    pub id: Uuid,
    pub github_login: String,
    pub name: Option<String>,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub projects: i64,
    pub builds: i64,
}

/// All users with their role and per-user project/build counts.
pub async fn users(
    State(app): State<AppState>,
    _admin: AdminUser,
) -> AppResult<Json<Vec<AdminUserRow>>> {
    let rows = sqlx::query_as::<_, AdminUserRow>(
        r#"SELECT
              u.id           AS id,
              u.github_login AS github_login,
              u.name         AS name,
              u.role         AS role,
              u.created_at   AS created_at,
              (SELECT count(*) FROM projects p WHERE p.user_id = u.id) AS projects,
              (SELECT count(*) FROM builds b
                 JOIN projects p ON p.id = b.project_id
                 WHERE p.user_id = u.id)                               AS builds
           FROM users u
           ORDER BY u.created_at"#,
    )
    .fetch_all(&app.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub struct SetRoleBody {
    pub role: String,
}

/// Promote/demote a user. Admins cannot change their own role (avoids
/// accidentally locking themselves out of the dashboard).
pub async fn set_role(
    State(app): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
    Json(body): Json<SetRoleBody>,
) -> AppResult<Json<AdminUserRow>> {
    let role = match body.role.as_str() {
        "user" | "admin" => body.role.as_str(),
        _ => return Err(AppError::bad_request("role must be 'user' or 'admin'")),
    };
    if admin.id == id {
        return Err(AppError::bad_request("you cannot change your own role"));
    }

    let res = sqlx::query("UPDATE users SET role = $1, updated_at = now() WHERE id = $2")
        .bind(role)
        .bind(id)
        .execute(&app.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    let row = sqlx::query_as::<_, AdminUserRow>(
        r#"SELECT
              u.id           AS id,
              u.github_login AS github_login,
              u.name         AS name,
              u.role         AS role,
              u.created_at   AS created_at,
              (SELECT count(*) FROM projects p WHERE p.user_id = u.id) AS projects,
              (SELECT count(*) FROM builds b
                 JOIN projects p ON p.id = b.project_id
                 WHERE p.user_id = u.id)                               AS builds
           FROM users u
           WHERE u.id = $1"#,
    )
    .bind(id)
    .fetch_one(&app.db)
    .await?;
    Ok(Json(row))
}
