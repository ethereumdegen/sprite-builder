use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::authz::{Capability, Role};

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: Uuid,
    pub github_id: i64,
    pub github_login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub github_token: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub role: String,
}

impl User {
    /// The user's role as a typed value (ADR 0016).
    pub fn role(&self) -> Role {
        Role::from_db(&self.role)
    }

    /// Capabilities derived from the user's role.
    pub fn capabilities(&self) -> &'static [Capability] {
        self.role().capabilities()
    }
}

/// Public view of a user (never leaks the GitHub token). Exposes the role and
/// its derived capabilities so the frontend can gate UI capability-by-capability.
#[derive(Debug, Serialize)]
pub struct UserPublic {
    pub id: Uuid,
    pub github_login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub role: String,
    pub capabilities: Vec<Capability>,
}

impl From<&User> for UserPublic {
    fn from(u: &User) -> Self {
        UserPublic {
            id: u.id,
            github_login: u.github_login.clone(),
            name: u.name.clone(),
            avatar_url: u.avatar_url.clone(),
            role: u.role().as_str().to_string(),
            capabilities: u.capabilities().to_vec(),
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Project {
    pub id: Uuid,
    #[serde(skip_serializing)]
    pub user_id: Uuid,
    pub name: String,
    pub repo_full_name: String,
    pub repo_id: Option<i64>,
    pub default_branch: String,
    pub dockerfile_path: String,
    pub container_port: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Build {
    pub id: Uuid,
    pub project_id: Uuid,
    pub commit_sha: String,
    pub status: String,
    pub sprite_name: Option<String>,
    pub url: Option<String>,
    pub logs: String,
    pub error: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// A Codespace: a long-lived sprite holding a git working tree for a project
/// (`/workspace/app`). Interactive file/bash/git ops run against the live sprite;
/// provisioning (sprite create + clone) is handled asynchronously by the worker.
/// Project-scoped; ownership is reached through the parent project (ADR 0003).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Codespace {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub branch: String,
    pub status: String,
    pub sprite_name: Option<String>,
    pub url: Option<String>,
    pub snapshot_key: Option<String>,
    pub logs: String,
    pub error: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// A Docuspace: an S3-backed file store (usually markdown) owned by a project.
/// There is no sprite or worker — files live as plain objects under the key
/// prefix `docuspaces/<id>/...`; this record is the only Postgres state.
/// Project-scoped; ownership is reached through the parent project (ADR 0003).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Docuspace {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A per-project environment variable, injected into the deployed container.
/// The `value` is returned to the owning user (reveal in the UI) but redacted
/// from build/runtime logs before they're persisted or served.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ProjectEnvVar {
    pub id: Uuid,
    #[serde(skip_serializing)]
    pub project_id: Uuid,
    pub key: String,
    pub value: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ApiKey {
    pub id: Uuid,
    #[serde(skip_serializing)]
    pub user_id: Uuid,
    pub name: String,
    #[serde(skip_serializing)]
    pub key_hash: String,
    pub key_prefix: String,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
