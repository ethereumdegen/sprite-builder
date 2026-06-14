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

    /// GitHub OAuth credentials. Only the API server (which serves the login
    /// flow) requires these; the background worker leaves them `None`.
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<String>,

    pub sprites_token: String,
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
    /// Config for the API server. The GitHub OAuth credentials are required
    /// because the server runs the login flow.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::load(true)
    }

    /// Config for the background build worker. The worker never serves the OAuth
    /// login flow, so the GitHub OAuth credentials are not required.
    pub fn from_env_worker() -> anyhow::Result<Self> {
        Self::load(false)
    }

    fn load(require_oauth: bool) -> anyhow::Result<Self> {
        // Required on the server, optional on the worker.
        let oauth = |key: &str| -> anyhow::Result<Option<String>> {
            if require_oauth {
                Ok(Some(env(key)?))
            } else {
                Ok(std::env::var(key).ok())
            }
        };

        Ok(Self {
            database_url: env("DATABASE_URL")?,
            // Platforms like Railway inject PORT and route traffic there, so
            // honor it first; then an explicit BIND_ADDR; then the local default.
            bind_addr: match std::env::var("PORT") {
                Ok(port) => format!("0.0.0.0:{port}"),
                Err(_) => env_or("BIND_ADDR", "0.0.0.0:8787"),
            },
            backend_url: env_or("BACKEND_URL", "http://localhost:8787"),
            frontend_url: env_or("FRONTEND_URL", "http://localhost:5173"),
            github_client_id: oauth("GITHUB_CLIENT_ID")?,
            github_client_secret: oauth("GITHUB_CLIENT_SECRET")?,
            sprites_token: env("SPRITES_TOKEN")?,
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

    /// The configured GitHub OAuth credentials `(client_id, client_secret)`.
    /// Errors if they weren't loaded — which only happens on a misconfigured
    /// server deploy, since `from_env` requires them.
    pub fn github_oauth(&self) -> anyhow::Result<(&str, &str)> {
        let id = self
            .github_client_id
            .as_deref()
            .context("GITHUB_CLIENT_ID not configured")?;
        let secret = self
            .github_client_secret
            .as_deref()
            .context("GITHUB_CLIENT_SECRET not configured")?;
        Ok((id, secret))
    }
}
