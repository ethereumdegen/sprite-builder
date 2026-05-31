use sprite_builder::config::Config;
use sprite_builder::{worker, AppState};

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
    let state = AppState::from_config(config).await?;

    worker::run(state).await
}
