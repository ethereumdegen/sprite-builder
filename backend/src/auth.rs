use axum::async_trait;
use axum::extract::{FromRef, FromRequestParts, Query, State};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Redirect};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{Duration, Utc};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::authz::Capability;
use crate::error::{AppError, AppResult};
use crate::models::{ApiKey, User, UserPublic};
use crate::{github, AppState};

const SESSION_COOKIE: &str = "sb_session";
const OAUTH_STATE_COOKIE: &str = "sb_oauth_state";
const SESSION_TTL_DAYS: i64 = 30;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

fn session_cookie(token: String, secure: bool) -> Cookie<'static> {
    let mut c = Cookie::new(SESSION_COOKIE, token);
    c.set_http_only(true);
    c.set_path("/");
    c.set_same_site(SameSite::Lax);
    c.set_secure(secure);
    c.set_max_age(time_duration_days(SESSION_TTL_DAYS));
    c
}

fn time_duration_days(days: i64) -> time::Duration {
    time::Duration::days(days)
}

// ---------------------------------------------------------------------------
// authenticated-user resolution (Bearer API key OR session cookie)
// ---------------------------------------------------------------------------

/// Resolve the calling user from either a Bearer API key (programmatic access)
/// or the browser session cookie (web UI). Shared by the auth extractors.
async fn resolve_user(parts: &Parts, app: &AppState) -> AppResult<User> {
    // 1) Bearer API key.
    if let Some(value) = parts.headers.get(AUTHORIZATION) {
        let value = value.to_str().unwrap_or_default();
        if let Some(token) = value.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                let hash = sha256_hex(token);
                let user = sqlx::query_as!(
                    User,
                    r#"SELECT u.id, u.github_id, u.github_login, u.name, u.avatar_url,
                              u.github_token, u.created_at, u.updated_at, u.role
                       FROM users u
                       JOIN api_keys k ON k.user_id = u.id
                       WHERE k.key_hash = $1"#,
                    hash,
                )
                .fetch_optional(&app.db)
                .await?;
                return match user {
                    Some(u) => {
                        let _ = sqlx::query!(
                            "UPDATE api_keys SET last_used_at = now() WHERE key_hash = $1",
                            hash,
                        )
                        .execute(&app.db)
                        .await;
                        Ok(u)
                    }
                    None => Err(AppError::Unauthorized),
                };
            }
        }
    }

    // 2) Session cookie.
    let jar = CookieJar::from_headers(&parts.headers);
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        let user = sqlx::query_as!(
            User,
            r#"SELECT u.id, u.github_id, u.github_login, u.name, u.avatar_url,
                      u.github_token, u.created_at, u.updated_at, u.role
               FROM users u
               JOIN sessions s ON s.user_id = u.id
               WHERE s.token = $1 AND s.expires_at > now()"#,
            cookie.value(),
        )
        .fetch_optional(&app.db)
        .await?;
        if let Some(u) = user {
            return Ok(u);
        }
    }

    Err(AppError::Unauthorized)
}

/// Extractor for any authenticated user (Bearer API key or session cookie).
pub struct AuthUser(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app = AppState::from_ref(state);
        Ok(AuthUser(resolve_user(parts, &app).await?))
    }
}

/// Extractor that additionally requires the admin dashboard capability
/// (ADR 0004 + 0016). Authorization is capability-based, never an inline role
/// check at the call site.
pub struct AdminUser(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for AdminUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app = AppState::from_ref(state);
        let user = resolve_user(parts, &app).await?;
        if user.role().has(Capability::ViewAdminDashboard) {
            Ok(AdminUser(user))
        } else {
            Err(AppError::Forbidden)
        }
    }
}

// ---------------------------------------------------------------------------
// GitHub OAuth
// ---------------------------------------------------------------------------

pub async fn github_login(
    State(app): State<AppState>,
    jar: CookieJar,
) -> AppResult<impl IntoResponse> {
    let (client_id, _) = app.config.github_oauth()?;
    let state = random_token(16);
    let scope = "read:user repo";
    let authorize = format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&state={}",
        urlencoding::encode(client_id),
        urlencoding::encode(&app.config.github_callback_url()),
        urlencoding::encode(scope),
        urlencoding::encode(&state),
    );

    let mut state_cookie = Cookie::new(OAUTH_STATE_COOKIE, state);
    state_cookie.set_http_only(true);
    state_cookie.set_path("/");
    state_cookie.set_same_site(SameSite::Lax);
    state_cookie.set_max_age(time::Duration::minutes(10));

    Ok((jar.add(state_cookie), Redirect::to(&authorize)))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: String,
    state: String,
}

pub async fn github_callback(
    State(app): State<AppState>,
    jar: CookieJar,
    Query(q): Query<CallbackQuery>,
) -> AppResult<(CookieJar, Redirect)> {
    // Verify the anti-CSRF state matches what we set.
    let expected = jar.get(OAUTH_STATE_COOKIE).map(|c| c.value().to_string());
    if expected.as_deref() != Some(q.state.as_str()) {
        return Err(AppError::bad_request("invalid oauth state"));
    }

    let (client_id, client_secret) = app.config.github_oauth()?;
    let token = github::exchange_code(
        &app.http,
        client_id,
        client_secret,
        &q.code,
        &app.config.github_callback_url(),
    )
    .await?;

    let gh_user = github::fetch_user(&app.http, &token).await?;

    // Seed-admin promotion (ADR 0016): an env-listed login is promoted to admin,
    // but a login dropping off the list never demotes an existing admin.
    let seed_admin = app.config.is_seed_admin(&gh_user.login);

    // Upsert the user, refreshing the stored token.
    let user = sqlx::query_as!(
        User,
        r#"INSERT INTO users (github_id, github_login, name, avatar_url, github_token, role)
           VALUES ($1, $2, $3, $4, $5, CASE WHEN $6 THEN 'admin' ELSE 'user' END)
           ON CONFLICT (github_id) DO UPDATE
             SET github_login = EXCLUDED.github_login,
                 name = EXCLUDED.name,
                 avatar_url = EXCLUDED.avatar_url,
                 github_token = EXCLUDED.github_token,
                 role = CASE WHEN $6 THEN 'admin' ELSE users.role END,
                 updated_at = now()
           RETURNING id, github_id, github_login, name, avatar_url,
                     github_token, created_at, updated_at, role"#,
        gh_user.id,
        gh_user.login,
        gh_user.name,
        gh_user.avatar_url,
        token,
        seed_admin,
    )
    .fetch_one(&app.db)
    .await?;

    // Create a session.
    let session_token = random_token(32);
    let expires = Utc::now() + Duration::days(SESSION_TTL_DAYS);
    sqlx::query!(
        "INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, $3)",
        session_token,
        user.id,
        expires,
    )
    .execute(&app.db)
    .await?;

    let secure = app.config.backend_url.starts_with("https");
    let jar = jar
        .remove(Cookie::from(OAUTH_STATE_COOKIE))
        .add(session_cookie(session_token, secure));

    Ok((jar, Redirect::to(&app.config.frontend_url)))
}

pub async fn logout(
    State(app): State<AppState>,
    jar: CookieJar,
) -> AppResult<(CookieJar, Json<serde_json::Value>)> {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        sqlx::query!("DELETE FROM sessions WHERE token = $1", cookie.value())
            .execute(&app.db)
            .await?;
    }
    Ok((
        jar.remove(Cookie::from(SESSION_COOKIE)),
        Json(serde_json::json!({ "ok": true })),
    ))
}

pub async fn me(AuthUser(user): AuthUser) -> Json<UserPublic> {
    Json(UserPublic::from(&user))
}

// ---------------------------------------------------------------------------
// API key management (web-session usage expected)
// ---------------------------------------------------------------------------

pub async fn list_keys(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
) -> AppResult<Json<Vec<ApiKey>>> {
    let keys = sqlx::query_as!(
        ApiKey,
        r#"SELECT id, user_id, name, key_hash, key_prefix, last_used_at, created_at
           FROM api_keys WHERE user_id = $1 ORDER BY created_at DESC"#,
        user.id,
    )
    .fetch_all(&app.db)
    .await?;
    Ok(Json(keys))
}

#[derive(Deserialize)]
pub struct CreateKeyBody {
    pub name: String,
}

pub async fn create_key(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateKeyBody>,
) -> AppResult<Json<serde_json::Value>> {
    if body.name.trim().is_empty() {
        return Err(AppError::bad_request("name is required"));
    }
    // Full key shown to the user exactly once.
    let secret = format!("sb_{}", random_token(24));
    let prefix = secret.chars().take(11).collect::<String>();
    let hash = sha256_hex(&secret);

    let key = sqlx::query_as!(
        ApiKey,
        r#"INSERT INTO api_keys (user_id, name, key_hash, key_prefix)
           VALUES ($1, $2, $3, $4)
           RETURNING id, user_id, name, key_hash, key_prefix, last_used_at, created_at"#,
        user.id,
        body.name.trim(),
        hash,
        prefix,
    )
    .fetch_one(&app.db)
    .await?;

    Ok(Json(serde_json::json!({
        "key": key,
        "secret": secret,        // only returned here, never again
    })))
}

pub async fn delete_key(
    State(app): State<AppState>,
    AuthUser(user): AuthUser,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let res = sqlx::query!(
        "DELETE FROM api_keys WHERE id = $1 AND user_id = $2",
        id,
        user.id,
    )
    .execute(&app.db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}
