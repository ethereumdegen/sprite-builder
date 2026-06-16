//! Codespace provisioning worker — an independent loop (decoupled from the build
//! loop) that turns a `queued` codespace into a live sprite holding a git clone.
//!
//! Mirrors the build worker's queue discipline (ADR 0006: `FOR UPDATE SKIP
//! LOCKED`, no broker) but does far less: create a sprite, clone the repo at the
//! codespace's branch into `/workspace/app`, set the commit identity, mark
//! `ready`. No Docker, no `docker run` — interactive file/exec/git work happens
//! later, synchronously, against the live sprite (see `crate::codespaces`).

use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{Codespace, Project, User};
use crate::AppState;

/// Cap on sprite provisioning (`POST /sprites`).
const CREATE_TIMEOUT: Duration = Duration::from_secs(120);

/// Cap on the clone exec — a large repo over the synchronous exec endpoint.
const CLONE_TIMEOUT: Duration = Duration::from_secs(600);

/// A codespace stuck in `provisioning` past this is considered abandoned (worker
/// crashed) and gets reaped. Must exceed CLONE_TIMEOUT.
const STALE_MINUTES: i32 = 30;

/// Friendly-name attempts before falling back to an id-suffixed name.
const MAX_NAME_ATTEMPTS: u32 = 8;

/// Final line of the clone script; its presence proves every prior step ran
/// (the script is `set -euo pipefail`), independent of exec HTTP framing.
const SUCCESS_MARKER: &str = "__CS_PROVISION_OK__";

/// Spawn the codespace provisioning loop + its reaper alongside the build loop.
pub fn spawn(app: AppState) {
    spawn_reaper(app.db.clone());
    tokio::spawn(async move {
        let poll = Duration::from_secs(app.config.worker_poll_secs.max(1));
        tracing::info!("codespace worker started (poll every {:?})", poll);
        loop {
            match claim_next(&app.db).await {
                Ok(Some(cs)) => {
                    tracing::info!("provisioning codespace {}", cs.id);
                    if let Err(e) = provision(&app, cs).await {
                        tracing::error!("codespace provision error: {e:#}");
                    }
                }
                Ok(None) => tokio::time::sleep(poll).await,
                Err(e) => {
                    tracing::error!("codespace poll error: {e:#}");
                    tokio::time::sleep(poll).await;
                }
            }
        }
    });
}

/// Atomically claim the oldest queued codespace (SKIP LOCKED — safe across worker
/// instances).
async fn claim_next(db: &PgPool) -> sqlx::Result<Option<Codespace>> {
    sqlx::query_as::<_, Codespace>(
        r#"
        WITH next AS (
            SELECT id FROM codespaces
            WHERE status = 'queued'
            ORDER BY created_at
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        UPDATE codespaces
        SET status = 'provisioning', started_at = now(), updated_at = now()
        FROM next
        WHERE codespaces.id = next.id
        RETURNING codespaces.id, codespaces.project_id, codespaces.name, codespaces.branch,
                  codespaces.status, codespaces.sprite_name, codespaces.url,
                  codespaces.snapshot_key, codespaces.logs, codespaces.error,
                  codespaces.metadata, codespaces.created_at, codespaces.updated_at,
                  codespaces.started_at, codespaces.finished_at
        "#,
    )
    .fetch_optional(db)
    .await
}

async fn provision(app: &AppState, cs: Codespace) -> anyhow::Result<()> {
    let project = sqlx::query_as::<_, Project>(
        r#"SELECT id, user_id, name, repo_full_name, repo_id, default_branch,
                  dockerfile_path, container_port, created_at
           FROM projects WHERE id = $1"#,
    )
    .bind(cs.project_id)
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

    let sprite = generate_sprite_name(&app.db, cs.id).await?;
    let mut log = format!("==> provisioning codespace on sprite {sprite}\n");
    sqlx::query("UPDATE codespaces SET sprite_name = $1, logs = $2, updated_at = now() WHERE id = $3")
        .bind(&sprite)
        .bind(&log)
        .bind(cs.id)
        .execute(&app.db)
        .await?;

    // Create the sprite (bounded so a wedged API surfaces in ~2 min).
    let url = match tokio::time::timeout(CREATE_TIMEOUT, app.sprites.create_sprite(&sprite)).await {
        Ok(Ok(api_url)) => api_url.unwrap_or_else(|| app.sprites.public_url(&sprite)),
        Ok(Err(e)) => return fail(app, cs.id, &sprite, &mut log, &format!("create sprite failed: {e}")).await,
        Err(_) => {
            let msg = format!("sprite did not provision within {}s", CREATE_TIMEOUT.as_secs());
            return fail(app, cs.id, &sprite, &mut log, &msg).await;
        }
    };
    log.push_str("==> sprite created; cloning repo\n");
    let _ = sqlx::query("UPDATE codespaces SET url = $1, logs = $2, updated_at = now() WHERE id = $3")
        .bind(&url)
        .bind(&log)
        .bind(cs.id)
        .execute(&app.db)
        .await;

    // Clone (synchronous, bounded). The token rides a credential helper, never
    // the clone URL, and is redacted from anything we store (ADR 0013).
    let script = clone_script(&project.repo_full_name, &cs.branch, &owner.github_login, &owner.github_token);
    let output = match tokio::time::timeout(CLONE_TIMEOUT, app.sprites.exec(&sprite, &script)).await {
        Ok(Ok(res)) => res.output,
        Ok(Err(e)) => return fail(app, cs.id, &sprite, &mut log, &format!("clone exec failed: {e}")).await,
        Err(_) => return fail(app, cs.id, &sprite, &mut log, &format!("clone timed out after {}s", CLONE_TIMEOUT.as_secs())).await,
    };
    log.push_str(&redact(&output, &owner.github_token));
    if !log.ends_with('\n') {
        log.push('\n');
    }

    if output.contains(SUCCESS_MARKER) {
        sqlx::query(
            r#"UPDATE codespaces
               SET status = 'ready', logs = $1, error = NULL,
                   finished_at = now(), updated_at = now()
               WHERE id = $2"#,
        )
        .bind(&log)
        .bind(cs.id)
        .execute(&app.db)
        .await?;
        tracing::info!("codespace {} ready on {}", cs.id, sprite);
        Ok(())
    } else {
        fail(app, cs.id, &sprite, &mut log, "clone did not complete (see logs)").await
    }
}

/// Mark a codespace failed, record the log + error, and tear down its sprite.
async fn fail(
    app: &AppState,
    id: Uuid,
    sprite: &str,
    log: &mut String,
    err: &str,
) -> anyhow::Result<()> {
    log.push_str(&format!("==> ERROR: {err}\n"));
    sqlx::query(
        r#"UPDATE codespaces
           SET status = 'failed', logs = $1, error = $2,
               finished_at = now(), updated_at = now()
           WHERE id = $3"#,
    )
    .bind(&*log)
    .bind(err)
    .bind(id)
    .execute(&app.db)
    .await?;
    let _ = app.sprites.delete_sprite(sprite).await;
    tracing::warn!("codespace {} failed: {}", id, err);
    Ok(())
}

/// The clone script. Tries the requested branch first, falling back to the repo's
/// default branch if it doesn't exist remotely.
fn clone_script(repo: &str, branch: &str, login: &str, token: &str) -> String {
    format!(
        r#"set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
export CS_GH_TOKEN='{token}'
HELPER='!f() {{ echo username=x-access-token; echo "password=$CS_GH_TOKEN"; }}; f'
echo "==> preparing /workspace"
rm -rf /workspace && mkdir -p /workspace && cd /workspace
echo "==> cloning {repo} (branch {branch})"
git -c credential.helper="$HELPER" clone --no-progress --branch "{branch}" "https://github.com/{repo}.git" app \
  || git -c credential.helper="$HELPER" clone --no-progress "https://github.com/{repo}.git" app
cd app
git config user.name '{login}'
git config user.email '{login}@users.noreply.github.com'
echo "==> HEAD: $(git rev-parse --short HEAD)"
echo "{marker}"
"#,
        token = token,
        repo = repo,
        branch = branch,
        login = login,
        marker = SUCCESS_MARKER,
    )
}

/// Scrub the GitHub token from anything we persist (defense in depth — the
/// credential helper keeps it out of the clone URL already).
fn redact(s: &str, token: &str) -> String {
    if token.is_empty() {
        s.to_string()
    } else {
        s.replace(token, "***")
    }
}

/// Generate a friendly, DNS-safe, unique sprite name for a codespace. The name is
/// the public subdomain, so it must be a valid DNS label; we retry on collision
/// with an existing codespace and fall back to an id-suffixed name.
async fn generate_sprite_name(db: &PgPool, id: Uuid) -> sqlx::Result<String> {
    // Generate the candidate names up front and drop the (non-Send) generator
    // before the first await, so this future stays Send for `tokio::spawn`.
    let candidates: Vec<String> = {
        let mut generator = names::Generator::default();
        (0..MAX_NAME_ATTEMPTS).filter_map(|_| generator.next()).collect()
    };
    for candidate in candidates {
        if !name_taken(db, &candidate).await? {
            return Ok(candidate);
        }
    }
    let suffix = &id.simple().to_string()[..6];
    Ok(format!("cs-{suffix}"))
}

async fn name_taken(db: &PgPool, name: &str) -> sqlx::Result<bool> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM codespaces WHERE sprite_name = $1 LIMIT 1")
            .bind(name)
            .fetch_optional(db)
            .await?;
    Ok(row.is_some())
}

/// Periodically fail codespaces stuck in `provisioning` (worker died mid-clone).
fn spawn_reaper(db: PgPool) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            match reap_stale(&db).await {
                Ok(n) if n > 0 => tracing::warn!("reaped {n} stale provisioning codespace(s)"),
                Ok(_) => {}
                Err(e) => tracing::error!("codespace reaper error: {e:#}"),
            }
        }
    });
}

async fn reap_stale(db: &PgPool) -> sqlx::Result<u64> {
    // STALE_MINUTES is a trusted compile-time constant (injection-safe to
    // interpolate; avoids binding an i64 into make_interval's int4 argument).
    let sql = format!(
        r#"UPDATE codespaces
           SET status = 'failed',
               error = 'provisioning orphaned: the worker stopped updating it (likely a worker restart mid-clone); marked stale by the reaper',
               finished_at = now(), updated_at = now()
           WHERE status = 'provisioning'
             AND started_at < now() - interval '{STALE_MINUTES} minutes'"#
    );
    Ok(sqlx::query(&sql).execute(db).await?.rows_affected())
}
