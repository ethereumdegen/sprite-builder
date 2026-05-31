use anyhow::{anyhow, Context};
use std::sync::Arc;

use crate::config::Config;

/// Thin client over the Sprites.dev REST API (https://docs.sprites.dev/api/).
#[derive(Clone)]
pub struct SpritesClient {
    http: reqwest::Client,
    config: Arc<Config>,
}

/// Result of running a single command inside a sprite.
pub struct ExecResult {
    pub status: reqwest::StatusCode,
    pub output: String,
}

impl ExecResult {
    pub fn ok(&self) -> bool {
        self.status.is_success()
    }
}

impl SpritesClient {
    pub fn new(http: reqwest::Client, config: Arc<Config>) -> Self {
        Self { http, config }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.config.sprites_api_base, path)
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.config.sprites_token)
    }

    /// Create a new sprite. POST /v1/sprites { "name": ... }
    pub async fn create_sprite(&self, name: &str) -> anyhow::Result<()> {
        let resp = self
            .http
            .post(self.url("/sprites"))
            .header("Authorization", self.bearer())
            .json(&serde_json::json!({ "name": name }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("create sprite failed ({status}): {body}"));
        }
        Ok(())
    }

    /// Run a command via the simple HTTP exec endpoint (non-TTY).
    /// POST /v1/sprites/{name}/exec?cmd=...&cmd=...  with optional stdin body.
    ///
    /// We always invoke through `bash -lc <script>` so callers can pass an
    /// arbitrary shell script as `script`.
    pub async fn exec(&self, sprite: &str, script: &str) -> anyhow::Result<ExecResult> {
        // cmd is repeatable: cmd=bash, cmd=-lc, cmd=<script>
        let query = [
            ("cmd", "bash"),
            ("cmd", "-lc"),
            ("cmd", script),
        ];
        let resp = self
            .http
            .post(self.url(&format!("/sprites/{sprite}/exec")))
            .query(&query)
            .header("Authorization", self.bearer())
            .send()
            .await
            .context("sprites exec request failed")?;
        let status = resp.status();
        let output = resp.text().await.unwrap_or_default();
        Ok(ExecResult { status, output })
    }

    /// Make the sprite's public URL reachable without sprite-org auth.
    /// Mirrors `sprite url update --auth public`.
    pub async fn set_url_public(&self, sprite: &str) -> anyhow::Result<()> {
        // The url settings live under the services/url resource; we PATCH auth=public.
        let resp = self
            .http
            .patch(self.url(&format!("/sprites/{sprite}/url")))
            .header("Authorization", self.bearer())
            .json(&serde_json::json!({ "auth": "public" }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            // Non-fatal: the build may still be reachable to org members.
            tracing::warn!("set_url_public failed ({status}): {body}");
        }
        Ok(())
    }

    /// The public URL of a sprite. Traffic is proxied to port 8080 inside the VM.
    /// Pattern: https://<sprite-name>-<org>.sprites.dev/
    pub fn public_url(&self, sprite: &str) -> String {
        format!("https://{}-{}.sprites.dev/", sprite, self.config.sprites_org)
    }
}
