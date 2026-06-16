//! Codespace provisioning worker — an independent loop (decoupled from the build
//! loop) that turns a `queued` codespace into a live, *empty* sprite.
//!
//! Mirrors the build worker's queue discipline (ADR 0006: `FOR UPDATE SKIP
//! LOCKED`, no broker) but does very little: create a sprite, make an empty
//! `/workspace/app`, mark `ready`. It deliberately does **not** clone a repo —
//! cloning is an explicit action the user/agent triggers later
//! (`POST /api/codespaces/:id/clone`), so provisioning stays fast and a slow or
//! failing clone can never wedge it. All interactive work (files/exec/git/clone)
//! runs synchronously against the live sprite (see `crate::codespaces`).

use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;

use crate::models::Codespace;
use crate::AppState;

/// Cap on sprite provisioning (`POST /sprites`).
const CREATE_TIMEOUT: Duration = Duration::from_secs(120);

/// Cap on the (trivial) workspace-setup exec.
const SETUP_TIMEOUT: Duration = Duration::from_secs(60);

/// A codespace stuck in `provisioning` past this is considered abandoned (worker
/// crashed) and gets reaped. Comfortably exceeds CREATE_TIMEOUT + SETUP_TIMEOUT.
const STALE_MINUTES: i32 = 30;

/// Friendly-name attempts before falling back to an id-suffixed name.
const MAX_NAME_ATTEMPTS: u32 = 8;

/// Final line of the setup script; its presence proves it ran to completion,
/// independent of exec HTTP framing.
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
    log.push_str("==> sprite created; preparing workspace\n");
    let _ = sqlx::query("UPDATE codespaces SET url = $1, logs = $2, updated_at = now() WHERE id = $3")
        .bind(&url)
        .bind(&log)
        .bind(cs.id)
        .execute(&app.db)
        .await;

    // Make an empty workspace. No clone — the user/agent clones explicitly later
    // (`POST /api/codespaces/:id/clone`). This trivial exec keeps provisioning
    // fast and unwedgeable.
    let script =
        format!("set -euo pipefail\nmkdir -p /workspace/app\necho \"{SUCCESS_MARKER}\"\n");
    let output = match tokio::time::timeout(SETUP_TIMEOUT, app.sprites.exec(&sprite, &script)).await
    {
        Ok(Ok(res)) => res.output,
        Ok(Err(e)) => return fail(app, cs.id, &sprite, &mut log, &format!("workspace setup failed: {e}")).await,
        Err(_) => return fail(app, cs.id, &sprite, &mut log, "workspace setup timed out").await,
    };

    if output.contains(SUCCESS_MARKER) {
        log.push_str(
            "==> workspace ready at /workspace/app (empty — clone a repo or write files via the API)\n",
        );
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
        fail(app, cs.id, &sprite, &mut log, "workspace setup did not complete").await
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
