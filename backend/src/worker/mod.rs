use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{Build, Project, User};
use crate::sprites::SpritesClient;
use crate::AppState;

/// Run the build worker loop forever. Intended to be the body of a dedicated
/// worker process (see `src/worker/main.rs`).
pub async fn run(app: AppState) -> anyhow::Result<()> {
    let poll = Duration::from_secs(app.config.worker_poll_secs.max(1));
    tracing::info!("build worker started (poll every {:?})", poll);
    loop {
        match claim_next(&app.db).await {
            Ok(Some(build)) => {
                tracing::info!("picked up build {}", build.id);
                if let Err(e) = run_build(&app, build).await {
                    tracing::error!("build run error: {e:#}");
                }
            }
            Ok(None) => tokio::time::sleep(poll).await,
            Err(e) => {
                tracing::error!("worker poll error: {e:#}");
                tokio::time::sleep(poll).await;
            }
        }
    }
}

/// Atomically claim the oldest queued build using SKIP LOCKED so multiple
/// worker instances never grab the same row.
async fn claim_next(db: &PgPool) -> sqlx::Result<Option<Build>> {
    let build = sqlx::query_as::<_, Build>(
        r#"
        WITH next AS (
            SELECT id FROM builds
            WHERE status = 'queued'
            ORDER BY created_at
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        UPDATE builds
        SET status = 'running', started_at = now(), updated_at = now()
        FROM next
        WHERE builds.id = next.id
        RETURNING builds.*
        "#,
    )
    .fetch_optional(db)
    .await?;
    Ok(build)
}

async fn run_build(app: &AppState, build: Build) -> anyhow::Result<()> {
    // Load the project + owner (for the GitHub clone token).
    let project = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1")
        .bind(build.project_id)
        .fetch_one(&app.db)
        .await?;
    let owner = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(project.user_id)
        .fetch_one(&app.db)
        .await?;

    let sprite_name = format!("build-{}", &build.id.simple().to_string()[..12]);
    sqlx::query("UPDATE builds SET sprite_name = $1, updated_at = now() WHERE id = $2")
        .bind(&sprite_name)
        .bind(build.id)
        .execute(&app.db)
        .await?;

    match build_on_sprite(&app.sprites, &sprite_name, &project, &owner, &build).await {
        Ok(logs) => {
            let url = app.sprites.public_url(&sprite_name);
            let _ = app.sprites.set_url_public(&sprite_name).await;
            let metadata = serde_json::json!({
                "sprite": sprite_name,
                "commit": build.commit_sha,
                "repo": project.repo_full_name,
                "container_port": project.container_port,
                "image": sprite_name,
            });
            finish(&app.db, build.id, "succeeded", Some(&url), &logs, None, metadata).await?;
            tracing::info!("build {} succeeded -> {}", build.id, url);
        }
        Err((logs, err)) => {
            finish(
                &app.db,
                build.id,
                "failed",
                None,
                &logs,
                Some(&err),
                serde_json::json!({ "sprite": sprite_name }),
            )
            .await?;
            tracing::warn!("build {} failed: {}", build.id, err);
        }
    }
    Ok(())
}

/// Provision the sprite and run the build script. On success returns the logs;
/// on failure returns (logs-so-far, error message).
async fn build_on_sprite(
    sprites: &SpritesClient,
    sprite_name: &str,
    project: &Project,
    owner: &User,
    build: &Build,
) -> Result<String, (String, String)> {
    let mut logs = String::new();

    macro_rules! note {
        ($($arg:tt)*) => {{ logs.push_str(&format!($($arg)*)); logs.push('\n'); }};
    }

    note!("==> creating sprite {sprite_name}");
    if let Err(e) = sprites.create_sprite(sprite_name).await {
        return Err((logs, format!("create sprite failed: {e}")));
    }

    let script = build_script(project, owner, build, sprite_name);
    note!("==> running build script");
    match sprites.exec(sprite_name, &script).await {
        Ok(res) => {
            logs.push_str(&res.output);
            if !logs.ends_with('\n') {
                logs.push('\n');
            }
            if res.ok() {
                note!("==> build script completed (exit ok)");
                Ok(logs)
            } else {
                Err((logs, format!("build script failed (http {})", res.status)))
            }
        }
        Err(e) => Err((logs, format!("exec failed: {e}"))),
    }
}

/// The bash script executed inside the sprite. It installs docker if needed,
/// clones the repo at the target commit, builds the image, and runs it mapped
/// to port 8080 (which the sprite proxies to its public URL).
fn build_script(project: &Project, owner: &User, build: &Build, image: &str) -> String {
    format!(
        r#"set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
echo "=== sprite-builder build {build_id} ==="

if ! command -v docker >/dev/null 2>&1; then
  echo "--- installing docker ---"
  curl -fsSL https://get.docker.com | sh
fi
if ! docker info >/dev/null 2>&1; then
  echo "--- starting dockerd ---"
  ( dockerd >/tmp/dockerd.log 2>&1 & ) || true
  for i in $(seq 1 30); do docker info >/dev/null 2>&1 && break; sleep 1; done
fi

rm -rf /workspace && mkdir -p /workspace && cd /workspace
echo "--- cloning {repo} @ {sha} ---"
git clone "https://x-access-token:{token}@github.com/{repo}.git" app
cd app
git checkout {sha}

echo "--- docker build ({dockerfile}) ---"
docker build -f "{dockerfile}" -t "{image}" .

echo "--- starting container on host port 8080 -> {port} ---"
docker rm -f "{image}" >/dev/null 2>&1 || true
docker run -d --name "{image}" --restart unless-stopped -p 8080:{port} "{image}"

echo "--- running containers ---"
docker ps
echo "=== done ==="
"#,
        build_id = build.id,
        repo = project.repo_full_name,
        sha = build.commit_sha,
        token = owner.github_token,
        dockerfile = project.dockerfile_path,
        image = image,
        port = project.container_port,
    )
}

#[allow(clippy::too_many_arguments)]
async fn finish(
    db: &PgPool,
    id: Uuid,
    status: &str,
    url: Option<&str>,
    logs: &str,
    error: Option<&str>,
    metadata: serde_json::Value,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"UPDATE builds
           SET status = $2, url = $3, logs = $4, error = $5, metadata = $6,
               finished_at = now(), updated_at = now()
           WHERE id = $1"#,
    )
    .bind(id)
    .bind(status)
    .bind(url)
    .bind(logs)
    .bind(error)
    .bind(metadata)
    .execute(db)
    .await?;
    Ok(())
}
