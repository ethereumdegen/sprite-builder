pub mod admin;
pub mod auth;
pub mod authz;
pub mod codespaces;
pub mod config;
pub mod error;
pub mod github;
pub mod models;
pub mod projects;
pub mod sprites;
pub mod worker;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{header, HeaderValue, Method};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use crate::config::Config;
use crate::sprites::SpritesClient;

/// Shared application state. `Clone` is cheap (pools/clients are reference-counted).
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub http: reqwest::Client,
    pub sprites: SpritesClient,
}

impl AppState {
    /// Build the full application state from config: create the (lazy) DB pool,
    /// the HTTP client, and wire up the sprites client.
    pub async fn from_config(config: Config) -> anyhow::Result<Self> {
        let config = Arc::new(config);

        // Lazy connect: build the pool without dialing Postgres so boot never
        // hangs/aborts if the DB is briefly unreachable (e.g. Railway's internal
        // network coming up after the app). Connections are established on first
        // use; `acquire_timeout` bounds that so a request-path query returns a
        // clean error (ADR-0012) instead of hanging forever.
        let db = PgPoolOptions::new()
            .max_connections(config.db_max_connections)
            .acquire_timeout(Duration::from_secs(10))
            .connect_lazy(&config.database_url)?;

        // A long timeout so docker builds inside sprites can run to completion
        // over the synchronous exec endpoint.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(900))
            .build()?;

        let sprites = SpritesClient::new(http.clone(), config.clone());

        Ok(Self {
            db,
            config,
            http,
            sprites,
        })
    }
}

/// Apply embedded SQL migrations.
pub async fn run_migrations(db: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(db).await?;
    Ok(())
}

/// Build the axum router (API routes + optional SPA static serving).
#[allow(clippy::expect_used)] // boot-phase carve-out (ADR 0010)
pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(
            state
                .config
                .frontend_url
                .parse::<HeaderValue>()
                // Boot-phase carve-out (ADR 0010): a misconfigured FRONTEND_URL
                // should fail fast at startup, not per request.
                .expect("invalid FRONTEND_URL"),
        )
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::PATCH,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    let api = Router::new()
        .route("/api/health", get(|| async { Json(serde_json::json!({ "ok": true })) }))
        // auth
        .route("/api/auth/github", get(auth::github_login))
        .route("/api/auth/github/callback", get(auth::github_callback))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/me", get(auth::me))
        // api keys
        .route("/api/keys", get(auth::list_keys).post(auth::create_key))
        .route("/api/keys/:id", delete(auth::delete_key))
        // repos + projects + builds
        .route("/api/repos", get(projects::list_repos))
        .route(
            "/api/projects",
            get(projects::list_projects).post(projects::create_project),
        )
        .route("/api/projects/:id", get(projects::get_project))
        .route(
            "/api/projects/:id/env",
            get(projects::list_env_vars).post(projects::upsert_env_var),
        )
        .route("/api/projects/:id/env/:key", delete(projects::delete_env_var))
        .route(
            "/api/projects/:id/builds",
            get(projects::list_builds).post(projects::create_build),
        )
        .route("/api/builds/:id", get(projects::get_build))
        .route("/api/builds/:id/runtime-logs", get(projects::runtime_logs))
        .route(
            "/api/builds/:id/url-visibility",
            get(projects::get_url_visibility).post(projects::set_url_visibility),
        )
        // codespaces (ephemeral coding filesystem) — decoupled from builds
        .route(
            "/api/projects/:id/codespaces",
            get(codespaces::list_codespaces).post(codespaces::create_codespace),
        )
        .route("/api/codespaces/:id", get(codespaces::get_codespace).delete(codespaces::delete_codespace))
        .route(
            "/api/codespaces/:id/files",
            get(codespaces::read_path)
                .put(codespaces::write_file)
                .delete(codespaces::delete_path),
        )
        .route("/api/codespaces/:id/exec", post(codespaces::exec))
        .route("/api/codespaces/:id/git", post(codespaces::git))
        // admin dashboard (capability-gated by the AdminUser extractor)
        .route("/api/admin/stats", get(admin::stats))
        .route("/api/admin/builds", get(admin::builds))
        .route("/api/admin/builds/:id/rebuild", post(admin::rebuild))
        .route("/api/admin/sprites", get(admin::sprites))
        .route("/api/admin/sprites/:name", delete(admin::delete_sprite))
        .route(
            "/api/admin/sprites/:name/public",
            post(admin::set_sprite_public),
        )
        .route("/api/admin/users", get(admin::users))
        .route("/api/admin/users/:id/role", patch(admin::set_role))
        .layer(cors)
        .with_state(state.clone());

    // Optionally serve the built frontend (SPA) with index.html fallback.
    let static_dir = &state.config.static_dir;
    if !static_dir.is_empty() && Path::new(static_dir).is_dir() {
        let index = format!("{static_dir}/index.html");
        let serve = ServeDir::new(static_dir).fallback(ServeFile::new(index));
        api.fallback_service(serve)
    } else {
        api
    }
}
