//! Admin dashboard endpoints: cross-tenant build/diagnostic visibility and user
//! role management. Every handler is gated by the [`AdminUser`] extractor, which
//! requires the admin capability (ADR 0004 + 0016) — there are no inline role
//! checks here.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

/// Re-run an existing build's *exact commit* as a fresh `queued` build that the
/// worker picks up. Reuses the stored `commit_sha` — no GitHub call — so an admin
/// can rebuild any user's sprite without needing that user's token. The worker
/// loads the project owner's token when it clones, so the build runs as the owner.
pub async fn rebuild(
    State(app): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<AdminBuild>> {
    let src = sqlx::query_as::<_, (Uuid, String)>(
        r#"SELECT project_id, commit_sha FROM builds WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(&app.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let new_id: (Uuid,) = sqlx::query_as(
        r#"INSERT INTO builds (project_id, commit_sha, status)
           VALUES ($1, $2, 'queued')
           RETURNING id"#,
    )
    .bind(src.0)
    .bind(src.1)
    .fetch_one(&app.db)
    .await?;

    let row = sqlx::query_as::<_, AdminBuild>(
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
           WHERE b.id = $1"#,
    )
    .bind(new_id.0)
    .fetch_one(&app.db)
    .await?;

    Ok(Json(row))
}

// ---------------------------------------------------------------------------
// sprites (live sprites.dev inventory)
// ---------------------------------------------------------------------------

/// One row in the admin Sprites index: a live sprite on sprites.dev, joined to
/// the build/project/owner that provisioned it (when we can identify one).
///
/// A sprite is a sprites.dev VM — lower level than our own `builds` abstraction,
/// which layers on top of one. `orphaned` flags a sprite that no longer maps to
/// any build we know about (left behind by a deleted build, a failed cleanup, or
/// created out-of-band) so it can be reclaimed.
#[derive(Serialize)]
pub struct AdminSprite {
    pub name: String,
    pub status: Option<String>,
    pub created_at: Option<String>,
    pub public_url: String,
    pub orphaned: bool,
    pub build_id: Option<Uuid>,
    pub build_status: Option<String>,
    pub project_id: Option<Uuid>,
    pub project_name: Option<String>,
    pub owner_login: Option<String>,
}

/// The most recent build that used a given sprite, keyed by sprite name.
#[derive(sqlx::FromRow)]
struct SpriteBuildRow {
    sprite_name: String,
    build_id: Uuid,
    build_status: String,
    project_id: Uuid,
    project_name: String,
    owner_login: String,
}

/// Every sprite currently live on sprites.dev, annotated with the build that
/// owns it. Reads through to the sprites.dev API (not our DB) for the source of
/// truth on what is actually running, then enriches with our own records.
pub async fn sprites(
    State(app): State<AppState>,
    _admin: AdminUser,
) -> AppResult<Json<Vec<AdminSprite>>> {
    let live = app.sprites.list_sprites().await?;

    // One query for the latest build per sprite name, then index by name.
    let names: Vec<String> = live.iter().map(|s| s.name.clone()).collect();
    let rows = sqlx::query_as::<_, SpriteBuildRow>(
        r#"SELECT DISTINCT ON (b.sprite_name)
              b.sprite_name    AS sprite_name,
              b.id             AS build_id,
              b.status         AS build_status,
              b.project_id     AS project_id,
              p.name           AS project_name,
              u.github_login   AS owner_login
           FROM builds b
           JOIN projects p ON p.id = b.project_id
           JOIN users u    ON u.id = p.user_id
           WHERE b.sprite_name = ANY($1)
           ORDER BY b.sprite_name, b.created_at DESC"#,
    )
    .bind(&names)
    .fetch_all(&app.db)
    .await?;
    let by_name: HashMap<String, SpriteBuildRow> =
        rows.into_iter().map(|r| (r.sprite_name.clone(), r)).collect();

    let out = live
        .into_iter()
        .map(|s| {
            let link = by_name.get(&s.name);
            AdminSprite {
                public_url: app.sprites.public_url(&s.name),
                orphaned: link.is_none(),
                build_id: link.map(|l| l.build_id),
                build_status: link.map(|l| l.build_status.clone()),
                project_id: link.map(|l| l.project_id),
                project_name: link.map(|l| l.project_name.clone()),
                owner_login: link.map(|l| l.owner_login.clone()),
                name: s.name,
                status: s.status,
                created_at: s.created_at,
            }
        })
        .collect();

    Ok(Json(out))
}

/// Tear down a sprite on sprites.dev. Useful for reclaiming orphaned or stuck
/// sprites directly from the dashboard.
///
/// Reconciles our own records: any build that provisioned this sprite no longer
/// has a live deployment, so flag it (`metadata.deployment_removed`) instead of
/// leaving a `succeeded` build pointing at a now-dead URL. The build page's live
/// probe is authoritative, but this keeps list views honest without a probe and
/// is the cheap fix for the "orphaned build row" the deletion would otherwise
/// create. The build's `status` is left untouched — the build still *succeeded*;
/// only the deployment is gone.
pub async fn delete_sprite(
    State(app): State<AppState>,
    _admin: AdminUser,
    Path(name): Path<String>,
) -> AppResult<StatusCode> {
    app.sprites.delete_sprite(&name).await?;
    sqlx::query(
        r#"UPDATE builds
           SET metadata = metadata || jsonb_build_object('deployment_removed', true),
               updated_at = now()
           WHERE sprite_name = $1"#,
    )
    .bind(&name)
    .execute(&app.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Make a sprite's public URL reachable without sprite-org auth (mirrors
/// `sprite url update --auth public`).
pub async fn set_sprite_public(
    State(app): State<AppState>,
    _admin: AdminUser,
    Path(name): Path<String>,
) -> AppResult<StatusCode> {
    app.sprites.set_url_public(&name).await?;
    Ok(StatusCode::NO_CONTENT)
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
