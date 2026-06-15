use std::time::{Duration, Instant};

use base64::Engine;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{Build, Project, User};
use crate::AppState;

/// Printed as the final line of the build script. Because the script runs under
/// `set -euo pipefail`, any failed step aborts before this line — so its presence
/// in the captured output is the source of truth for build success (independent
/// of however the exec endpoint maps process exit codes to HTTP status).
const SUCCESS_MARKER: &str = "__SB_BUILD_OK__";

/// Marker the poll command appends once the detached build has exited; followed
/// by the build's exit code.
const DONE_MARKER: &str = "__SB_DONE__:";

/// How long to wait for the deployed app to start serving on its sprite URL.
const READINESS_TIMEOUT: Duration = Duration::from_secs(90);

/// Max wall-clock for a single build before we give up polling it.
const BUILD_TIMEOUT: Duration = Duration::from_secs(1200);

/// How often we poll the in-sprite build log.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// A `running` build older than this is considered abandoned (worker crashed)
/// and gets reaped. Must exceed BUILD_TIMEOUT.
const STALE_MINUTES: i32 = 30;

// ---------------------------------------------------------------------------
// worker entrypoint
// ---------------------------------------------------------------------------

/// Run the build worker loop forever. Intended to be the body of a dedicated
/// worker process (see `src/worker/main.rs`).
pub async fn run(app: AppState) -> anyhow::Result<()> {
    spawn_reaper(app.db.clone());

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

/// Periodically fail builds stuck in `running` (e.g. a worker died).
fn spawn_reaper(db: PgPool) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            match reap_stale(&db).await {
                Ok(n) if n > 0 => tracing::warn!("reaped {n} stale running build(s)"),
                Ok(_) => {}
                Err(e) => tracing::error!("reaper error: {e:#}"),
            }
        }
    });
}

async fn reap_stale(db: &PgPool) -> sqlx::Result<u64> {
    // STALE_MINUTES is a trusted compile-time constant, so interpolating it into
    // the interval literal is injection-safe and avoids binding an i64 into
    // make_interval's int4 argument.
    let sql = format!(
        r#"UPDATE builds
           SET status = 'failed',
               error = 'build did not finish in time (worker lost or timed out); marked stale',
               finished_at = now(), updated_at = now()
           WHERE status = 'running'
             AND started_at < now() - interval '{STALE_MINUTES} minutes'"#
    );
    Ok(sqlx::query(&sql).execute(db).await?.rows_affected())
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
        RETURNING builds.id, builds.project_id, builds.commit_sha, builds.status,
                  builds.sprite_name, builds.url, builds.logs, builds.error,
                  builds.metadata, builds.created_at, builds.updated_at,
                  builds.started_at, builds.finished_at
        "#,
    )
    .fetch_optional(db)
    .await?;
    Ok(build)
}

// ---------------------------------------------------------------------------
// build execution
// ---------------------------------------------------------------------------

async fn run_build(app: &AppState, build: Build) -> anyhow::Result<()> {
    let project = sqlx::query_as::<_, Project>(
        r#"SELECT id, user_id, name, repo_full_name, repo_id, default_branch,
                  dockerfile_path, container_port, created_at
           FROM projects WHERE id = $1"#,
    )
    .bind(build.project_id)
    .fetch_one(&app.db)
    .await?;
    let owner = sqlx::query_as::<_, User>(
        r#"SELECT id, github_id, github_login, name, avatar_url, github_token,
                  created_at, updated_at, role
           FROM users WHERE id = $1"#,
    )
    .bind(project.user_id)
    .fetch_one(&app.db)
    .await?;

    let sprite_name = format!("build-{}", &build.id.simple().to_string()[..12]);
    sqlx::query("UPDATE builds SET sprite_name = $1, updated_at = now() WHERE id = $2")
        .bind(&sprite_name)
        .bind(build.id)
        .execute(&app.db)
        .await?;

    let base_meta = |ready: serde_json::Value| {
        serde_json::json!({
            "sprite": sprite_name,
            "commit": build.commit_sha,
            "repo": project.repo_full_name,
            "container_port": project.container_port,
            "image": sprite_name,
            "ready": ready,
        })
    };

    match run_on_sprite(app, &sprite_name, &project, &owner, &build).await {
        Ok(logs) => {
            // Built OK. Promote it: make the URL public and confirm it serves (#3).
            let _ = app.sprites.set_url_public(&sprite_name).await;
            let url = app.sprites.public_url(&sprite_name);

            if probe_until_ready(&app.http, &url).await {
                finish(&app.db, build.id, "succeeded", Some(&url), &logs, None, base_meta(true.into())).await?;
                tracing::info!("build {} succeeded -> {}", build.id, url);
                // #5 blue-green: tear down this project's older deployments.
                cleanup_prior_sprites(app, project.id, &sprite_name).await;
            } else {
                let logs = format!(
                    "{logs}\n==> app did not become reachable at {url} within {}s",
                    READINESS_TIMEOUT.as_secs()
                );
                finish(
                    &app.db, build.id, "failed", Some(&url), &logs,
                    Some("image built, but the app did not become reachable on the sprite URL"),
                    base_meta(false.into()),
                ).await?;
                tracing::warn!("build {} built but not reachable", build.id);
                let _ = app.sprites.delete_sprite(&sprite_name).await; // keep prior deployment live
            }
        }
        Err((logs, err)) => {
            finish(&app.db, build.id, "failed", None, &logs, Some(&err), base_meta(false.into())).await?;
            tracing::warn!("build {} failed: {}", build.id, err);
            let _ = app.sprites.delete_sprite(&sprite_name).await;
        }
    }
    Ok(())
}

/// Create the sprite, ship + launch the build script detached, and stream its
/// log file into the DB until it finishes. Returns redacted logs on success, or
/// (redacted logs, error) on failure.
async fn run_on_sprite(
    app: &AppState,
    sprite: &str,
    project: &Project,
    owner: &User,
    build: &Build,
) -> Result<String, (String, String)> {
    let token = owner.github_token.clone();
    let mut head = format!("==> creating sprite {sprite}\n");
    let _ = update_logs(&app.db, build.id, &head).await;

    if let Err(e) = app.sprites.create_sprite(sprite).await {
        return Err((head, format!("create sprite failed: {e}")));
    }

    // Ship the build script as base64 (avoids any shell-quoting hazards).
    let script = build_script(project, build, &token, sprite);
    let b64 = base64::engine::general_purpose::STANDARD.encode(script.as_bytes());
    let write_cmd = format!("echo '{b64}' | base64 -d > /root/sb-build.sh && echo wrote");
    if let Err(e) = exec_ok(app, sprite, &write_cmd).await {
        head.push_str("==> failed to upload build script\n");
        return Err((head, format!("upload script failed: {e}")));
    }

    // Launch detached so we can poll the log without holding a long HTTP request.
    let launch = r#"rm -f /var/log/sb-build.log /var/log/sb-build.done
setsid bash -c 'bash /root/sb-build.sh >/var/log/sb-build.log 2>&1; echo $? >/var/log/sb-build.done' </dev/null >/dev/null 2>&1 &
echo launched"#;
    if let Err(e) = exec_ok(app, sprite, launch).await {
        head.push_str("==> failed to launch build\n");
        return Err((head, format!("launch failed: {e}")));
    }
    head.push_str("==> build started\n");

    // Poll the log file until the build records an exit code (or we time out).
    let poll_cmd = format!(
        "cat /var/log/sb-build.log 2>/dev/null || true\n\
         if [ -f /var/log/sb-build.done ]; then printf '\\n{DONE_MARKER}'; cat /var/log/sb-build.done; fi"
    );
    let deadline = Instant::now() + BUILD_TIMEOUT;
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        let raw = match app.sprites.exec(sprite, &poll_cmd).await {
            Ok(res) => res.output,
            Err(_) => continue, // transient; retry until deadline
        };
        let (body, exit) = parse_poll(&raw);
        let logs = format!("{head}{}", redact(&body, &token));
        let _ = update_logs(&app.db, build.id, &logs).await;

        if let Some(code) = exit {
            if code == 0 && body.contains(SUCCESS_MARKER) {
                return Ok(logs);
            }
            return Err((logs, format!("build script exited with code {code}")));
        }
        if Instant::now() >= deadline {
            let logs = format!("{logs}\n==> timed out after {}s", BUILD_TIMEOUT.as_secs());
            return Err((logs, "build timed out".into()));
        }
    }
}

/// The bash script run inside the sprite. Uses a git credential helper (token in
/// an env var, never in the clone URL) so a failed clone can't echo the token.
fn build_script(project: &Project, build: &Build, token: &str, image: &str) -> String {
    format!(
        r#"set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
echo "==> sprite-builder build {build_id}"
echo "==> commit {sha}"

if ! command -v docker >/dev/null 2>&1; then
  echo "==> installing docker"
  curl -fsSL https://get.docker.com | sh
fi
if ! docker info >/dev/null 2>&1; then
  echo "==> starting dockerd"
  ( dockerd >/tmp/dockerd.log 2>&1 & ) || true
  for i in $(seq 1 30); do docker info >/dev/null 2>&1 && break; sleep 1; done
fi

export SB_GH_TOKEN='{token}'
rm -rf /workspace && mkdir -p /workspace && cd /workspace
echo "==> cloning {repo} @ {sha}"
git -c credential.helper='!f() {{ echo username=x-access-token; echo "password=$SB_GH_TOKEN"; }}; f' \
    clone --no-progress "https://github.com/{repo}.git" app
cd app
git checkout {sha}

echo "==> docker build ({dockerfile})"
docker build -f "{dockerfile}" -t "{image}" .

echo "==> starting container (host :8080 -> :{port})"
docker rm -f "{image}" >/dev/null 2>&1 || true
docker run -d --name "{image}" --restart unless-stopped -p 8080:{port} "{image}"
docker ps
echo "{marker}"
"#,
        build_id = build.id,
        sha = build.commit_sha,
        token = token,
        repo = project.repo_full_name,
        dockerfile = project.dockerfile_path,
        image = image,
        port = project.container_port,
        marker = SUCCESS_MARKER,
    )
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Run a command and require an HTTP-success response.
async fn exec_ok(app: &AppState, sprite: &str, cmd: &str) -> anyhow::Result<()> {
    let res = app.sprites.exec(sprite, cmd).await?;
    if res.ok() {
        Ok(())
    } else {
        anyhow::bail!("exec returned http {}: {}", res.status, res.output)
    }
}

/// Split poll output into (log body, optional exit code).
fn parse_poll(output: &str) -> (String, Option<i32>) {
    if let Some(idx) = output.rfind(DONE_MARKER) {
        let code = output[idx + DONE_MARKER.len()..]
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .parse::<i32>()
            .ok();
        (output[..idx].trim_end().to_string(), code)
    } else {
        (output.to_string(), None)
    }
}

/// Defense-in-depth: scrub the GitHub token from anything we persist.
fn redact(s: &str, token: &str) -> String {
    if token.is_empty() {
        s.to_string()
    } else {
        s.replace(token, "***")
    }
}

async fn update_logs(db: &PgPool, id: Uuid, logs: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE builds SET logs = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(logs)
        .execute(db)
        .await?;
    Ok(())
}

/// #3 — poll the public URL until it serves (any non-5xx response) or we time out.
async fn probe_until_ready(http: &reqwest::Client, url: &str) -> bool {
    let deadline = Instant::now() + READINESS_TIMEOUT;
    loop {
        if let Ok(resp) = http.get(url).timeout(Duration::from_secs(8)).send().await {
            if resp.status().as_u16() < 500 {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
}

/// #5 — delete every sprite this project used except the one we want to keep.
async fn cleanup_prior_sprites(app: &AppState, project_id: Uuid, keep: &str) {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"SELECT DISTINCT sprite_name
           FROM builds
           WHERE project_id = $1 AND sprite_name IS NOT NULL AND sprite_name <> $2"#,
    )
    .bind(project_id)
    .bind(keep)
    .fetch_all(&app.db)
    .await
    .unwrap_or_default();
    for (name,) in rows {
        tracing::info!("cleaning up old sprite {name}");
        let _ = app.sprites.delete_sprite(&name).await;
    }
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
