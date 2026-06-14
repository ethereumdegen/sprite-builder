use axum::{routing::get, Json, Router};
use sprite_builder::config::Config;
use sprite_builder::{run_migrations, worker, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    // The worker doesn't serve the OAuth login flow, so it skips the GitHub
    // OAuth env vars (GITHUB_CLIENT_ID / GITHUB_CLIENT_SECRET).
    let config = Config::from_env_worker()?;
    let bind_addr = config.bind_addr.clone();
    let state = AppState::from_config(config).await?;

    // The worker is otherwise headless, but Railway (and similar platforms)
    // health-check an HTTP endpoint on $PORT and will kill a deploy that never
    // answers. Run a tiny liveness server next to the build loop so the platform
    // sees the worker as up. It shares $PORT via the same bind_addr the server
    // uses (PORT -> BIND_ADDR -> default), so no extra config is needed. Bind it
    // before migrations, since migrations can block on the Postgres advisory
    // lock if the server is migrating at the same time.
    spawn_health_server(bind_addr);

    // #9 — run migrations here too. sqlx takes a Postgres advisory lock during
    // migration, so the server and worker starting together is safe (one waits),
    // and the worker never queries tables before they exist.
    run_migrations(&state.db).await?;

    worker::run(state).await
}

/// Minimal liveness endpoint so platform health checks pass. Binding failures
/// are logged but non-fatal — the build loop is what actually matters.
fn spawn_health_server(bind_addr: String) {
    tokio::spawn(async move {
        let app = Router::new().route(
            "/api/health",
            get(|| async { Json(serde_json::json!({ "ok": true, "service": "worker" })) }),
        );
        match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(listener) => {
                tracing::info!("worker health server listening on http://{bind_addr}");
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("worker health server error: {e}");
                }
            }
            Err(e) => tracing::error!("worker health server failed to bind {bind_addr}: {e}"),
        }
    });
}
