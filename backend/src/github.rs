use anyhow::{anyhow, Context};
use serde::Deserialize;

const USER_AGENT: &str = "sprite-builder";

#[derive(Debug, Deserialize)]
pub struct GithubUser {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct GithubRepo {
    pub id: i64,
    pub full_name: String,
    pub name: String,
    pub private: bool,
    pub default_branch: String,
    pub description: Option<String>,
    pub html_url: String,
    pub updated_at: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error_description: Option<String>,
}

/// Exchange an OAuth `code` for a user access token.
pub async fn exchange_code(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> anyhow::Result<String> {
    let resp: TokenResponse = http
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .await?
        .json()
        .await
        .context("decoding github token response")?;

    resp.access_token
        .ok_or_else(|| anyhow!(resp.error_description.unwrap_or_else(|| "no access_token returned".into())))
}

pub async fn fetch_user(http: &reqwest::Client, token: &str) -> anyhow::Result<GithubUser> {
    let resp = http
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .context("github GET /user failed")?;
    Ok(resp.json().await?)
}

/// List repositories the authenticated user can access (most recently updated first).
pub async fn list_repos(http: &reqwest::Client, token: &str) -> anyhow::Result<Vec<GithubRepo>> {
    let mut all = Vec::new();
    for page in 1..=5 {
        let repos: Vec<GithubRepo> = http
            .get("https://api.github.com/user/repos")
            .query(&[
                ("per_page", "100"),
                ("sort", "updated"),
                ("affiliation", "owner,collaborator,organization_member"),
                ("page", &page.to_string()),
            ])
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()
            .context("github GET /user/repos failed")?
            .json()
            .await?;
        let len = repos.len();
        all.extend(repos);
        if len < 100 {
            break;
        }
    }
    Ok(all)
}

/// Resolve the latest commit SHA for a given branch (or the repo's default branch HEAD).
pub async fn latest_commit_sha(
    http: &reqwest::Client,
    token: &str,
    full_name: &str,
    branch: &str,
) -> anyhow::Result<String> {
    #[derive(Deserialize)]
    struct CommitRef {
        sha: String,
    }
    let url = format!("https://api.github.com/repos/{full_name}/commits/{branch}");
    let commit: CommitRef = http
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("github GET {url} failed"))?
        .json()
        .await?;
    Ok(commit.sha)
}
