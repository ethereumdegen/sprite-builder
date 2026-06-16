use std::str::FromStr;

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

    /// GitHub logins (lowercased) seeded as admins on login. Lets you bootstrap
    /// the first admin without touching the database. Promotes only — removing a
    /// login here never demotes an already-admin user.
    pub admin_github_logins: Vec<String>,

    /// S3-compatible object storage backing Docuspaces (DigitalOcean Spaces, AWS
    /// S3, MinIO, …). All five are optional; when unset the Docuspace endpoints
    /// return a clean "S3 is not configured" 400 (`crate::storage::build_bucket`).
    pub s3_endpoint: Option<String>,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_access_key: Option<String>,
    pub s3_secret_key: Option<String>,
}

fn env(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var {key}"))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Parse an optional, typed env var, failing fast on a malformed value
/// (ADR 0005) instead of silently falling back to the default.
fn env_parse<T>(key: &str, default: T) -> anyhow::Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    match std::env::var(key) {
        Ok(raw) => raw
            .parse::<T>()
            .map_err(|e| anyhow::anyhow!("invalid value for {key} ({raw:?}): {e}")),
        Err(_) => Ok(default),
    }
}

fn parse_admin_logins(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
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
            worker_poll_secs: env_parse("WORKER_POLL_SECS", 5u64)?,
            db_max_connections: env_parse("DB_MAX_CONNECTIONS", 10u32)?,
            static_dir: env_or("STATIC_DIR", ""),
            admin_github_logins: parse_admin_logins(&env_or("ADMIN_GITHUB_LOGINS", "")),
            s3_endpoint: std::env::var("S3_ENDPOINT").ok(),
            s3_region: env_or("S3_REGION", "us-east-1"),
            s3_bucket: env_or("S3_BUCKET", "docuspaces"),
            s3_access_key: std::env::var("S3_ACCESS_KEY").ok(),
            s3_secret_key: std::env::var("S3_SECRET_KEY").ok(),
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

    /// Whether a GitHub login should be seeded as an admin (case-insensitive).
    pub fn is_seed_admin(&self, login: &str) -> bool {
        let login = login.to_lowercase();
        self.admin_github_logins.contains(&login)
    }
}
