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

    let state = AppState::from_config(config).await?;
    run_migrations(&state.db).await?;

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("api server listening on http://{bind_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
