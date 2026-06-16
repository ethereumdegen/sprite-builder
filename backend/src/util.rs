//! Small, dependency-free helpers shared by the file-oriented sub-applications
//! (codespaces, docuspaces). Kept deliberately neutral — nothing here knows about
//! sprites, S3, or any one module's storage — so either side can depend on it
//! without depending on the other.

use crate::error::{AppError, AppResult};

/// Validate a caller-supplied relative path and return it cleaned. Rejects
/// absolute paths, `~`, NUL bytes, and any `..` traversal segment, so the result
/// is always safe to join under a fixed root (a workspace dir or an S3 prefix).
///
/// With `allow_root`, an empty/`.` path returns `""` (meaning "the root");
/// otherwise an empty path is an error.
pub fn validate_rel_path(path: &str, allow_root: bool) -> AppResult<String> {
    let p = path.trim().trim_start_matches("./").to_string();
    if p.is_empty() || p == "." {
        if allow_root {
            return Ok(String::new());
        }
        return Err(AppError::bad_request("a file path is required"));
    }
    if p.starts_with('/') || p.starts_with('~') {
        return Err(AppError::bad_request("path must be relative to the root"));
    }
    if p.contains('\0') {
        return Err(AppError::bad_request("invalid path"));
    }
    if p.split('/').any(|seg| seg == "..") {
        return Err(AppError::bad_request("path may not traverse outside the root"));
    }
    Ok(p)
}

/// A random adjective-noun label (e.g. `selfish-change`) from the `names` crate —
/// the same source as the sprite subdomain — used as a default name for a newly
/// created resource. Created and dropped in one expression so the non-`Send`
/// generator never crosses an await.
pub fn random_name() -> String {
    names::Generator::default()
        .next()
        .unwrap_or_else(|| "untitled".to_string())
}
