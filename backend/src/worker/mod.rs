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

/// Marker the poll command appends every tick with a liveness probe — the count
/// of running build-script processes. Lets us tell "running, no output yet" from
/// "the detached build died" without polluting the user-visible log.
const STAT_MARKER: &str = "__SB_STAT__:";

/// Consecutive polls showing the build process gone (with no recorded exit code)
/// before we declare it dead. A couple ticks of tolerance avoids a false alarm in
/// the brief window between the script exiting and the done-file being written.
const MAX_DEAD_POLLS: u32 = 3;

/// How long to wait for the deployed app to start serving on its sprite URL.
const READINESS_TIMEOUT: Duration = Duration::from_secs(90);

/// Max wall-clock for a single build before we give up polling it.
const BUILD_TIMEOUT: Duration = Duration::from_secs(1200);

/// How often we poll the in-sprite build log.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Consecutive failed log polls before we declare the sprite unreachable and
/// fail the build. Without this, transient errors are swallowed forever and a
/// wedged exec channel only surfaces at BUILD_TIMEOUT (20 min) with no logs.
const MAX_POLL_ERRORS: u32 = 10;

/// Cap on sprite provisioning (`POST /sprites`). Without this the call rides the
/// global 900s HTTP timeout, so a wedged provision freezes the build on
/// "creating sprite" for 15 min with no signal.
const CREATE_TIMEOUT: Duration = Duration::from_secs(120);

/// Cap on a single setup exec (upload script / launch). The build itself runs
/// detached and is bounded by BUILD_TIMEOUT, not this.
const EXEC_TIMEOUT: Duration = Duration::from_secs(60);

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
               error = 'build orphaned: the worker stopped updating it (likely a worker restart/redeploy mid-build); marked stale by the reaper',
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

    let sprite_name = generate_sprite_name(&app.db, build.id).await?;
    // Surface the public URL immediately. It's derived purely from the sprite
    // name, so it's known the moment we pick the name — no need to wait for the
    // build to finish. It won't actually serve until the container is up and the
    // URL is made public at the end; the separate "Reachable" indicator tracks
    // that. finish() rewrites url to its final value (kept on success, cleared on
    // build failure).
    let url = app.sprites.public_url(&sprite_name);
    sqlx::query("UPDATE builds SET sprite_name = $1, url = $2, updated_at = now() WHERE id = $3")
        .bind(&sprite_name)
        .bind(&url)
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
            // `url` was already computed and persisted up front (it's derived from
            // the sprite name); reuse it here.
            let _ = app.sprites.set_url_public(&sprite_name).await;

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
    let mut head = format!("==> creating sprite {sprite} (provisioning VM)\n");
    let _ = update_logs(&app.db, build.id, &head).await;

    // Bound provisioning so a wedged Sprites API surfaces in ~2 min, not 15.
    match tokio::time::timeout(CREATE_TIMEOUT, app.sprites.create_sprite(sprite)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err((head, format!("create sprite failed: {e}"))),
        Err(_) => {
            let msg = format!("sprite did not provision within {}s", CREATE_TIMEOUT.as_secs());
            head.push_str(&format!("==> {msg}\n"));
            return Err((head, msg));
        }
    }
    // Persist each phase as it happens, so the log panel reflects progress
    // instead of freezing on "creating sprite" until the first poll lands.
    head.push_str("==> sprite created; uploading build script\n");
    let _ = update_logs(&app.db, build.id, &head).await;

    // Ship the build script as base64 (avoids any shell-quoting hazards).
    let script = build_script(project, build, &token, sprite);
    let b64 = base64::engine::general_purpose::STANDARD.encode(script.as_bytes());
    let write_cmd = format!("echo '{b64}' | base64 -d > /root/sb-build.sh && echo wrote");
    if let Err(e) = exec_ok(app, sprite, &write_cmd).await {
        head.push_str("==> failed to upload build script\n");
        return Err((head, format!("upload script failed: {e}")));
    }

    // Run the build as the foreground command of a keepalive session, then drop
    // the connection. Sprites kills non-TTY execs 10s after disconnect by
    // default, so we pass a long max_run_after_disconnect (= BUILD_TIMEOUT) and
    // poll its log file from separate short execs. Record the shell PID so each
    // poll can probe liveness with `kill -0`.
    let run = r#"rm -f /var/log/sb-build.log /var/log/sb-build.done /var/log/sb-build.pid
echo $$ >/var/log/sb-build.pid
bash /root/sb-build.sh >/var/log/sb-build.log 2>&1
echo $? >/var/log/sb-build.done"#;
    if let Err(e) = app.sprites.exec_detached(sprite, run, BUILD_TIMEOUT).await {
        head.push_str("==> failed to launch build\n");
        return Err((head, format!("launch failed: {e}")));
    }
    head.push_str("==> build started; streaming logs\n");
    let _ = update_logs(&app.db, build.id, &head).await;

    // Poll the log file until the build records an exit code (or we time out).
    // Each tick also reports a liveness count (running sb-build.sh processes) so
    // an empty log can be diagnosed as "starting up" vs "the build died".
    let poll_cmd = format!(
        "cat /var/log/sb-build.log 2>/dev/null || true\n\
         printf '\\n{STAT_MARKER}%s' \"$(kill -0 \"$(cat /var/log/sb-build.pid 2>/dev/null)\" 2>/dev/null && echo 1 || echo 0)\"\n\
         if [ -f /var/log/sb-build.done ]; then printf '\\n{DONE_MARKER}'; cat /var/log/sb-build.done; fi"
    );
    let started = Instant::now();
    let deadline = started + BUILD_TIMEOUT;
    let mut poll_errors = 0u32;
    let mut dead_polls = 0u32;
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        // Bound each poll: the shared HTTP client's timeout is 900s, so without
        // this a single wedged exec freezes the whole loop for ~15 min with no
        // output and no heartbeat — which is exactly what dockerd starting inside
        // the sprite triggers (it rewrites iptables and can sever the exec
        // channel). Treat a timed-out poll as a poll error so it surfaces in ~60s
        // and the build fails fast with a real cause instead of riding the 30-min
        // stale reaper.
        let outcome = match tokio::time::timeout(EXEC_TIMEOUT, app.sprites.exec(sprite, &poll_cmd))
            .await
        {
            Ok(Ok(res)) => Ok(res.output),
            Ok(Err(e)) => Err(format!("{e:#}")),
            Err(_) => Err(format!(
                "log poll exec timed out after {}s (sprite exec channel unresponsive)",
                EXEC_TIMEOUT.as_secs()
            )),
        };
        let raw = match outcome {
            Ok(out) => {
                poll_errors = 0;
                out
            }
            Err(e) => {
                // Don't swallow it: log it, surface it in the panel, and give up
                // after enough consecutive failures rather than spinning silently.
                poll_errors += 1;
                tracing::warn!(
                    "build {} log poll failed ({poll_errors}/{MAX_POLL_ERRORS}): {e}",
                    build.id
                );
                if poll_errors >= MAX_POLL_ERRORS {
                    let logs = format!(
                        "{head}==> lost contact with sprite while streaming logs: {e}\n"
                    );
                    let _ = update_logs(&app.db, build.id, &logs).await;
                    return Err((logs, format!("sprite unreachable after {poll_errors} failed log polls: {e}")));
                }
                continue;
            }
        };
        let (body, alive, exit) = parse_poll(&raw);
        let body = redact(&body, &token);

        // Show real output as it streams; when there's none yet, show a heartbeat
        // (elapsed + process state) instead of a frozen blank panel.
        let logs = if body.trim().is_empty() && exit.is_none() {
            let state = match alive {
                Some(true) => "build process running",
                Some(false) => "build process not running",
                None => "build process state unknown",
            };
            format!(
                "{head}==> waiting for build output… ({}s elapsed; {state})\n",
                started.elapsed().as_secs()
            )
        } else {
            format!("{head}{body}\n")
        };
        let _ = update_logs(&app.db, build.id, &logs).await;

        if let Some(code) = exit {
            if code == 0 && body.contains(SUCCESS_MARKER) {
                return Ok(logs);
            }
            return Err((logs, format!("build script exited with code {code}")));
        }

        // The detached build vanished without recording an exit code — almost
        // always means the sprite didn't keep the background process alive past
        // the launch exec. Fail fast with a clear cause instead of timing out.
        if alive == Some(false) {
            dead_polls += 1;
            if dead_polls >= MAX_DEAD_POLLS {
                let logs = format!(
                    "{logs}==> build process exited without recording a result \
                     (the sprite may not keep background processes alive across exec calls)\n"
                );
                let _ = update_logs(&app.db, build.id, &logs).await;
                return Err((logs, "build process died without recording an exit code".into()));
            }
        } else {
            dead_polls = 0;
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
# Stream the docker build as plain step-by-step text (not the animated TTY UI),
# so every layer/step lands in the polled log file like a Railway build log.
export DOCKER_BUILDKIT=1
export BUILDKIT_PROGRESS=plain
echo "==> sprite-builder build {build_id}"
echo "==> commit {sha}"

# The sprite exec session runs as an unprivileged user (uid 1001 `sprite`), not
# root — even though $HOME/`/root` happen to be writable. `dockerd` does a hard
# geteuid()==0 check and refuses to start otherwise ("dockerd needs to be
# started with root privileges"), so the daemon and the installer must go
# through `sudo` (passwordless on the sprite). Once the daemon is up we relax the
# socket perms so the rest of this script's `docker` client calls work as the
# sprite user without sudo.
if ! command -v docker >/dev/null 2>&1; then
  echo "==> installing docker"
  curl -fsSL https://get.docker.com | sudo sh
fi
if ! docker info >/dev/null 2>&1; then
  echo "==> starting dockerd"
  ( sudo dockerd >/tmp/dockerd.log 2>&1 & ) || true
  for i in $(seq 1 30); do sudo docker info >/dev/null 2>&1 && break; sleep 1; done
  # If the daemon never came up, surface *why* (its startup log) and fail here
  # with a clear cause, instead of letting `docker build` hit the opaque
  # "cannot connect to /var/run/docker.sock" a few lines down.
  if ! sudo docker info >/dev/null 2>&1; then
    echo "==> ERROR: dockerd did not become ready within 30s. /tmp/dockerd.log follows:"
    echo "-------------------- dockerd.log --------------------"
    cat /tmp/dockerd.log 2>/dev/null || echo "(no /tmp/dockerd.log was written — dockerd may have failed to exec)"
    echo "-----------------------------------------------------"
    exit 1
  fi
  # dockerd creates the socket root-owned; make it usable by the sprite user so
  # the plain `docker` calls below don't each need sudo.
  sudo chmod 666 /var/run/docker.sock || true
fi

export SB_GH_TOKEN='{token}'
rm -rf /workspace && mkdir -p /workspace && cd /workspace
echo "==> cloning {repo} @ {sha}"
git -c credential.helper='!f() {{ echo username=x-access-token; echo "password=$SB_GH_TOKEN"; }}; f' \
    clone --no-progress "https://github.com/{repo}.git" app
cd app
git checkout {sha}

# Work around a sandbox/procfs bug: BuildKit reads /proc/meminfo with a small
# multi-read pattern, and the sprite's sandboxed procfs returns a torn read for
# that pattern — fields concatenated with no newlines — which BuildKit rejects
# ("failed to solve: Internal: error parsing file: Malformed line ..."). A single
# large read (`cat`) is always clean, so snapshot meminfo once and bind-mount the
# static copy over /proc/meminfo for the build (we hold CAP_SYS_ADMIN). This keeps
# full BuildKit support rather than falling back to the legacy builder.
cat /proc/meminfo > /tmp/meminfo.static
sudo mount --bind /tmp/meminfo.static /proc/meminfo || echo "==> warning: could not pin /proc/meminfo; build may hit the meminfo parser bug"

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

/// How many friendly names to try before falling back to a guaranteed-unique one.
const MAX_NAME_ATTEMPTS: u32 = 8;

/// Generate a friendly, DNS-safe, unique sprite name for a build.
///
/// The sprite name *is* the public subdomain (`https://<name>-<org>.sprites.dev/`),
/// so instead of `build-<id>` we use the `names` crate's random adjective-noun
/// pairs (e.g. `selfish-change`) — lowercase + hyphen, which is both a valid DNS
/// label and a valid Docker image tag / container name (used verbatim in
/// `build_script`). We retry on the rare collision with an existing build's
/// sprite name; the final fallback appends a slice of the build id so this can
/// never fail to produce a unique name.
async fn generate_sprite_name(db: &PgPool, build_id: Uuid) -> sqlx::Result<String> {
    let mut generator = names::Generator::default();
    for _ in 0..MAX_NAME_ATTEMPTS {
        // `Generator::next` only yields `None` if its word lists are empty, which
        // the bundled defaults never are; fall back defensively rather than panic.
        let Some(candidate) = generator.next() else { break };
        if !sprite_name_taken(db, &candidate).await? {
            return Ok(candidate);
        }
    }
    // Guaranteed-unique fallback: a friendly pair plus a build-id suffix.
    let suffix = &build_id.simple().to_string()[..4];
    let base = names::Generator::default()
        .next()
        .unwrap_or_else(|| "build".to_string());
    Ok(format!("{base}-{suffix}"))
}

/// Whether any build already uses this sprite name (live or historical).
async fn sprite_name_taken(db: &PgPool, name: &str) -> sqlx::Result<bool> {
    let row: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM builds WHERE sprite_name = $1 LIMIT 1")
        .bind(name)
        .fetch_optional(db)
        .await?;
    Ok(row.is_some())
}

/// Run a command and require an HTTP-success response. Bounded by EXEC_TIMEOUT
/// so a hung setup exec fails the build fast instead of riding the 900s HTTP cap.
async fn exec_ok(app: &AppState, sprite: &str, cmd: &str) -> anyhow::Result<()> {
    let res = tokio::time::timeout(EXEC_TIMEOUT, app.sprites.exec(sprite, cmd))
        .await
        .map_err(|_| anyhow::anyhow!("exec timed out after {}s", EXEC_TIMEOUT.as_secs()))??;
    if res.ok() {
        Ok(())
    } else {
        anyhow::bail!("exec returned http {}: {}", res.status, res.output)
    }
}

/// Split poll output into (log body, build-process-alive flag, exit code).
/// The poll appends `STAT_MARKER<n>` (process count) every tick and, once done,
/// `DONE_MARKER<code>`. Both are stripped from the returned body. `alive` is
/// `None` if the probe was missing/unparseable.
fn parse_poll(output: &str) -> (String, Option<bool>, Option<i32>) {
    let mut rest = output;

    let mut exit = None;
    if let Some(idx) = rest.rfind(DONE_MARKER) {
        exit = rest[idx + DONE_MARKER.len()..]
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .parse::<i32>()
            .ok();
        rest = &rest[..idx];
    }

    let mut alive = None;
    if let Some(idx) = rest.rfind(STAT_MARKER) {
        alive = rest[idx + STAT_MARKER.len()..]
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .parse::<i32>()
            .ok()
            .map(|n| n > 0);
        rest = &rest[..idx];
    }

    (rest.trim_end().to_string(), alive, exit)
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
