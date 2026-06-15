//! Role- and capability-based authorization (ADR 0016).
//!
//! Authorization is expressed as *capabilities*, which are derived from a user's
//! role in exactly one place — [`Role::capabilities`]. Handlers never compare
//! role strings inline (ADR 0004); they gate access through typed extractors
//! (see [`crate::auth::AdminUser`]) that check for a required capability.

use std::fmt;

use serde::Serialize;

/// A coarse user role, persisted on `users.role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Admin,
}

impl Role {
    /// Parse the stored role string. Unknown values degrade to the
    /// least-privileged role rather than failing the request.
    pub fn from_db(s: &str) -> Self {
        match s {
            "admin" => Role::Admin,
            _ => Role::User,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }

    /// Capabilities granted to this role. This is the single source of truth for
    /// the role -> capability mapping (ADR 0016: scopes derived from roles).
    pub fn capabilities(self) -> &'static [Capability] {
        match self {
            Role::Admin => &[Capability::ViewAdminDashboard, Capability::ManageUsers],
            Role::User => &[],
        }
    }

    /// Whether this role grants `cap`.
    pub fn has(self, cap: Capability) -> bool {
        self.capabilities().contains(&cap)
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A discrete capability a request may require. Serialized to snake_case so the
/// frontend can gate UI on capabilities (not on the raw role string).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Read the cross-tenant admin dashboard (all builds / diagnostics).
    ViewAdminDashboard,
    /// Change other users' roles.
    ManageUsers,
}
