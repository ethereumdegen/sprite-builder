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
    let state = AppState::from_config(config).await?;

    // #9 — run migrations here too. sqlx takes a Postgres advisory lock during
    // migration, so the server and worker starting together is safe (one waits),
    // and the worker never queries tables before they exist.
    run_migrations(&state.db).await?;

    worker::run(state).await
}
