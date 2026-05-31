use anyhow::Context;

/// Runtime configuration, loaded from environment variables.
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,

    /// Public base URL of this backend (used to build the OAuth callback).
    pub backend_url: String,
    /// Public base URL of the frontend (where we redirect after login).
    pub frontend_url: String,

    pub github_client_id: String,
    pub github_client_secret: String,

    pub sprites_token: String,
    pub sprites_api_base: String,
    /// Sprites org slug, used to construct the public `<sprite>-<org>.sprites.dev` URL.
    pub sprites_org: String,

    pub worker_poll_secs: u64,
    pub db_max_connections: u32,
    /// Directory of built frontend assets to serve (empty = don't serve a SPA).
    pub static_dir: String,
}

fn env(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var {key}"))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: env("DATABASE_URL")?,
            bind_addr: env_or("BIND_ADDR", "0.0.0.0:8787"),
            backend_url: env_or("BACKEND_URL", "http://localhost:8787"),
            frontend_url: env_or("FRONTEND_URL", "http://localhost:5173"),
            github_client_id: env("GITHUB_CLIENT_ID")?,
            github_client_secret: env("GITHUB_CLIENT_SECRET")?,
            sprites_token: env("SPRITES_TOKEN")?,
            sprites_api_base: env_or("SPRITES_API_BASE", "https://api.sprites.dev/v1"),
            sprites_org: env("SPRITES_ORG")?,
            worker_poll_secs: env_or("WORKER_POLL_SECS", "5")
                .parse()
                .unwrap_or(5),
            db_max_connections: env_or("DB_MAX_CONNECTIONS", "10")
                .parse()
                .unwrap_or(10),
            static_dir: env_or("STATIC_DIR", ""),
        })
    }

    pub fn github_callback_url(&self) -> String {
        format!("{}/api/auth/github/callback", self.backend_url)
    }
}
