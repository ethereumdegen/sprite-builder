use sprite_builder::config::Config;
use sprite_builder::{build_router, run_migrations, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let config = Config::from_env()?;
    let bind_addr = config.bind_addr.clone();

    // Lazy pool (see AppState::from_config) — this never dials Postgres, so the
    // listener below comes up immediately and the platform healthcheck
    // (GET /api/health, no DB) passes without waiting on the database.
    let state = AppState::from_config(config).await?;

    // Run migrations off the critical path so a slow/late DB doesn't delay the
    // healthcheck. A genuine migration failure still aborts the process (loud,
    // crash-looping deploy per ADR-0005), rather than silently serving against
    // an unmigrated schema.
    let migrate_state = state.clone();
    tokio::spawn(async move {
        match run_migrations(&migrate_state.db).await {
            Ok(()) => tracing::info!("database migrations applied"),
            Err(e) => {
                tracing::error!("database migrations failed: {e:#}");
                std::process::exit(1);
            }
        }
    });

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("api server listening on http://{bind_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
