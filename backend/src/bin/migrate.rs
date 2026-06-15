use sprite_builder::run_migrations;
use sqlx::postgres::PgPoolOptions;

/// Standalone migration runner: `cargo run --bin migrate`.
/// Only needs DATABASE_URL (handy for CI / deploy release steps).
#[tokio::main]
#[allow(clippy::expect_used)] // boot-phase carve-out (ADR 0010)
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt().with_env_filter("info").init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let db = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await?;

    run_migrations(&db).await?;
    tracing::info!("migrations applied");
    Ok(())
}
