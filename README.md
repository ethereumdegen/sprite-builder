# 🛠️ Sprite Builder

Sign in with GitHub, pick a repo, click **New build** (or hit the API) — a
background worker spins up a [sprites.dev](https://sprites.dev) sandbox, builds
your repo's Docker image at a given commit, runs it, and gives you back a live
URL. Everything is recorded in Postgres (NeonDB).

```
GitHub login ──▶ create Project (pick a repo) ──▶ trigger Build (HEAD or a sha)
                                                        │
                                              queued row in Postgres
                                                        │
                                                build worker claims it
                                                        │
                            sprites.dev: create sprite ▶ clone ▶ docker build ▶ docker run :8080
                                                        │
                              update build: status=succeeded, url=https://<sprite>-<org>.sprites.dev
```

## Stack

| Layer     | Tech                                                        |
|-----------|-------------------------------------------------------------|
| Backend   | Rust, [axum](https://github.com/tokio-rs/axum), `sqlx`      |
| Frontend  | React + Vite + TypeScript                                   |
| Database  | NeonDB (Postgres)                                           |
| Sandboxes | [sprites.dev](https://sprites.dev) REST API                 |
| Auth      | GitHub OAuth (web sessions) + bearer **API keys** (the API) |

## Layout

```
sprite-builder/
├── backend/                  # Rust workspace (one lib, three bins)
│   ├── src/
│   │   ├── lib.rs            # AppState, router, migrations (shared by all bins)
│   │   ├── main.rs          # bin: sprite-builder        (API server)
│   │   ├── worker/
│   │   │   ├── mod.rs        # build worker loop
│   │   │   └── main.rs       # bin: sprite-builder-worker (background worker)
│   │   ├── bin/migrate.rs    # bin: migrate              (run migrations)
│   │   ├── auth.rs           # GitHub OAuth, sessions, AuthUser/AdminUser extractors
│   │   ├── authz.rs          # roles -> capabilities (ADR 0016)
│   │   ├── admin.rs          # admin dashboard routes (capability-gated)
│   │   ├── github.rs         # GitHub REST client
│   │   ├── sprites.rs        # sprites.dev REST client
│   │   ├── projects.rs       # projects / repos / builds routes
│   │   ├── codespaces.rs     # codespaces subapp: files / exec / git (live sprite)
│   │   ├── worker/
│   │   │   └── codespaces.rs # codespace provisioning loop (clone into a sprite)
│   │   ├── models.rs         # DB models
│   │   ├── config.rs · error.rs
│   │   └── migrations/       # 0001_init.sql … 0004_codespaces.sql
│   ├── .sqlx/               # committed offline query cache (ADR 0002)
│   ├── clippy.toml · deny.toml  # lint/dependency enforcement
├── frontend/                 # React + Vite SPA
│   ├── src/stores/          # per-domain Zustand stores (ADR 0007)
│   └── eslint.config.js     # frontend ADR enforcement
├── enforcement/              # adr-checks.sh + ADR->mechanism map
├── Dockerfile · railway.toml # API server (also serves the built SPA)
├── worker/                   # worker Dockerfile + railway.toml
└── dev.sh                    # run server + worker + frontend locally
```

The **API server** and the **build worker** are separate binaries/processes that
share the same `lib` and database — so you can scale or deploy them
independently (mirrors the `../devops-agent` layout).

## How auth works

- **Web UI** authenticates via **GitHub OAuth**; the backend stores a session
  and sets an `sb_session` cookie. The user's GitHub token is stored server-side
  and used to list repos, resolve commits, and clone during a build.
- **The API** is gated by **bearer API keys**. Create one in the *API Keys* page;
  the secret (`sb_…`) is shown once and only its SHA-256 hash is stored.

Every `/api/*` route accepts **either** a session cookie **or**
`Authorization: Bearer <api-key>`, so the same endpoints power the UI and
programmatic access.

## Roles & the admin dashboard

Every user has a **role** — `user` (default) or `admin`. Authorization is
**capability-based**: a role maps to a set of capabilities (`backend/src/authz.rs`),
and protected routes are gated by a typed `AdminUser` extractor that checks for
the required capability — never an inline role-string comparison.

Admins get an **`/admin` dashboard** with app-wide visibility: live counts,
every build job across all users (filterable by status, with owner/project/commit
and error diagnostics), and a user list where they can promote/demote others.

**Bootstrapping the first admin:** set `ADMIN_GITHUB_LOGINS` to a comma-separated
list of GitHub logins. Those users are promoted to `admin` on their next login.
It only ever *promotes* — dropping a login from the list never demotes someone.
After that, admins can manage roles from the dashboard. (You can also flip a role
directly: `UPDATE users SET role='admin' WHERE github_login='…';`.)

## Setup

### 1. NeonDB

Create a project at [neon.tech](https://neon.tech) and grab the connection
string (it looks like `postgres://…neon.tech/neondb?sslmode=require`).

### 2. GitHub OAuth App

Create one at <https://github.com/settings/developers>:

- **Homepage URL:** `http://localhost:5173`
- **Authorization callback URL:** `http://localhost:5173/api/auth/github/callback`

> In dev the Vite server proxies `/api` to the backend, so the browser only ever
> talks to `:5173` — that keeps the session cookie same-origin. Set `BACKEND_URL`
> to the origin the **browser** hits (`:5173` in dev), not the backend's `:8787`.

Scopes requested: `read:user repo`.

### 3. sprites.dev token

Create a token at <https://sprites.dev/account> (or `sprite org auth`). Note your
org slug — the public build URL is `https://<sprite-name>-<org>.sprites.dev`.

### 4. Env

```bash
cp backend/.env.example .env     # dev.sh reads ./.env at the repo root
$EDITOR .env                      # fill in DATABASE_URL, GITHUB_*, SPRITES_*
```

### 5. Run it

```bash
./dev.sh                 # starts API server + build worker + Vite frontend
# ./dev.sh --no-worker   # skip the worker
```

Open <http://localhost:5173>. (First Rust build takes a few minutes.)

Run migrations explicitly if you want: `cd backend && cargo run --bin migrate`.
The server also auto-applies them on startup.

## Configuration (environment variables)

All config is read from the environment (loaded from `.env` in dev). The
**server** needs the full set. The **worker** never serves the OAuth login flow,
so it needs only `DATABASE_URL`, `SPRITES_TOKEN`, and `SPRITES_ORG` (plus the
optional `WORKER_POLL_SECS` / `DB_MAX_CONNECTIONS`) — it does **not** need
`GITHUB_CLIENT_ID` / `GITHUB_CLIENT_SECRET`. The **migrate** bin only needs
`DATABASE_URL`.

| Variable               | Required | Default                      | Used by            | Description                                                                 |
|------------------------|:--------:|------------------------------|--------------------|-----------------------------------------------------------------------------|
| `DATABASE_URL`         | ✅       | —                            | server, worker, migrate | NeonDB/Postgres connection string (use `?sslmode=require`).            |
| `GITHUB_CLIENT_ID`     | ✅       | —                            | server             | GitHub OAuth app client ID.                                                 |
| `GITHUB_CLIENT_SECRET` | ✅       | —                            | server             | GitHub OAuth app client secret.                                             |
| `SPRITES_TOKEN`        | ✅       | —                            | server, worker     | sprites.dev API token (`sprites.dev/account` or `sprite org auth`).         |
| `SPRITES_ORG`          | ✅       | —                            | server, worker     | sprites org slug; builds the public URL `https://<sprite>-<org>.sprites.dev`. |
| `BACKEND_URL`          |          | `http://localhost:5173`      | server             | Origin the **browser** hits; base for the OAuth callback. In dev = `:5173` (Vite proxies `/api`). In prod = your public https origin. |
| `FRONTEND_URL`         |          | `http://localhost:5173`      | server             | Post-login redirect target and the CORS allow-origin.                       |
| `BIND_ADDR`            |          | `0.0.0.0:8787`               | server             | Address/port the API server listens on.                                     |
| `WORKER_POLL_SECS`     |          | `5`                          | worker             | How often the worker polls for queued builds.                               |
| `DB_MAX_CONNECTIONS`   |          | `10`                         | server, worker     | Postgres pool size per process.                                             |
| `ADMIN_GITHUB_LOGINS`  |          | _(empty)_                    | server             | Comma-separated GitHub logins auto-promoted to `admin` on login (promote-only). |
| `STATIC_DIR`           |          | _(empty)_                    | server             | Dir of built SPA assets to serve. Empty in dev (Vite serves it); set to `/app/static` in the Docker image. |

> `WORKER_POLL_SECS` and `DB_MAX_CONNECTIONS` fail fast on a malformed value
> rather than silently falling back to the default (ADR 0005).

> ⚠️ The OAuth callback registered on the GitHub app must be exactly
> `<BACKEND_URL>/api/auth/github/callback` — so `http://localhost:5173/api/auth/github/callback`
> in dev, and `https://<your-domain>/api/auth/github/callback` in prod.

## Using the API

```bash
KEY=sb_xxxxxxxxxxxxxxxxxxxxxxxx
BASE=http://localhost:5173      # or your deployed origin

# list projects
curl -H "Authorization: Bearer $KEY" $BASE/api/projects

# create a project for a repo
curl -X POST -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' \
  -d '{"name":"my-app","repo_full_name":"me/my-app","default_branch":"main","container_port":8080}' \
  $BASE/api/projects

# trigger a build (omit commit_sha to build HEAD of the default branch)
curl -X POST -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' \
  -d '{}' $BASE/api/projects/<project-id>/builds

# poll a build
curl -H "Authorization: Bearer $KEY" $BASE/api/builds/<build-id>
```

### Endpoints

| Method | Path                              | Purpose                              |
|--------|-----------------------------------|--------------------------------------|
| GET    | `/api/health`                     | health check                         |
| GET    | `/api/auth/github`                | start GitHub OAuth (web)             |
| GET    | `/api/auth/github/callback`       | OAuth callback                       |
| POST   | `/api/auth/logout`                | clear session                        |
| GET    | `/api/me`                         | current user                         |
| GET    | `/api/repos`                      | the user's GitHub repos              |
| GET/POST | `/api/projects`                 | list / create projects               |
| GET    | `/api/projects/:id`               | get a project                        |
| GET/POST | `/api/projects/:id/builds`      | list / trigger builds                |
| GET    | `/api/builds/:id`                 | build status, logs, url, metadata    |
| GET/POST | `/api/projects/:id/codespaces`  | list / create codespaces             |
| GET/DELETE | `/api/codespaces/:id`           | codespace status / destroy           |
| GET/PUT/DELETE | `/api/codespaces/:id/files`  | read-or-list / write / delete a path |
| POST   | `/api/codespaces/:id/exec`        | run a bash command in the workspace  |
| POST   | `/api/codespaces/:id/git`         | `{op: status\|diff\|commit\|push\|pull}` |
| GET/POST | `/api/keys`                     | list / create API keys              |
| DELETE | `/api/keys/:id`                   | revoke an API key                    |
| GET    | `/api/admin/stats`                | **admin** — app-wide counts          |
| GET    | `/api/admin/builds`               | **admin** — all builds (`?status=&limit=`) |
| GET    | `/api/admin/users`                | **admin** — users with role + counts |
| PATCH  | `/api/admin/users/:id/role`       | **admin** — set a user's role        |

> `/api/admin/*` routes require the admin capability; non-admins get `403`.

## How a build runs (the worker)

For each `queued` build the worker:

1. claims the row atomically (`FOR UPDATE SKIP LOCKED`),
2. creates a sprite (`POST /v1/sprites`),
3. uploads a build script and launches it **detached**, then polls its log file
   and streams output into `builds.logs` (so the UI shows **live logs**). The
   script: `git clone` the repo at the target commit → `docker build` the
   `Dockerfile` → `docker run -d -p 8080:<container_port>`,
4. treats the build as successful only when the script's exit code is `0` **and**
   a success marker is present — not merely because the HTTP call returned 200,
5. makes the sprite URL public and **probes it until it serves** (a non-5xx
   response) before marking `succeeded`; if it never responds, the build is
   `failed` with `metadata.ready=false`,
6. on success, tears down the project's **older** sprites (blue-green: one live
   deployment per project); on failure, deletes the just-created sprite.

Two more safety nets: a **reaper** fails builds stuck in `running` for too long
(crashed worker), and the GitHub token is passed via a **git credential helper**
(never in the clone URL) and **redacted** from stored logs.

**sprites.dev proxies external traffic to port `8080` inside the VM**, so the
worker maps the container's port to host `8080`. Set a project's `container_port`
to whatever your app listens on (default `8080`).

## Codespaces (ephemeral coding filesystem)

A **Codespace** is a separate subapp — think GitHub Codespaces, or a worktree +
git living in a sandbox. It's decoupled from the build system: builds turn a repo
into a running image; a codespace gives you a *live, editable working tree* you
(or an agent) can read, write, run commands in, and push back to GitHub.

```
project ──▶ create Codespace ──▶ [worker] create sprite + git clone into /workspace/app ──▶ ready
                                                  │
   interactive, synchronous (against the live sprite via the exec API):
   ├─ list/read/write/delete files under /workspace/app   (path-jailed)
   ├─ run arbitrary bash                                   ("as if local")
   └─ git status / diff / commit / push / pull             (token via credential helper)
```

- **Provisioning** is asynchronous, on the same Postgres queue discipline as
  builds (`FOR UPDATE SKIP LOCKED`) but in its **own worker loop**
  (`worker/codespaces.rs`) — independent of the build queue. It creates a sprite,
  clones the project's **default branch** into `/workspace/app` with the owner's
  GitHub token (via a git credential helper, never in the clone URL), sets the
  commit identity (`<github_login>` / `<login>@users.noreply.github.com`), and
  marks the codespace `ready`. Lifecycle: `queued → provisioning → ready | failed`.
- **Files / exec / git** run **synchronously** against the live sprite from the
  API server (the same bounded-exec pattern as build runtime logs). Everything the
  caller sends — paths, file contents, the exec command, commit messages, the
  token — is **base64-encoded and decoded inside the sprite**, so nothing can
  break out of its shell variable. Filesystem ops are **jailed** under
  `/workspace/app` (rejected client-side, then re-checked in-sprite with
  `realpath` to defeat symlink escapes), and project secrets are redacted from
  returned output.
- The **sprite stays alive** as the codespace's filesystem (v1). There is no
  hibernation yet — see *Roadmap* below.

The `exec` endpoint is fully live but has no terminal box in the v1 UI (it's
API-only, for agents/scripts); the web UI exposes the file browser/editor and the
commit/push panel.

### Roadmap (not built yet)

- **Phase 2 — hibernation.** Snapshot `/workspace` to S3/DigitalOcean Spaces on
  *stop* and restore it onto a fresh sprite on *start*, so idle codespaces cost
  nothing. The `codespaces.snapshot_key` column and the `stopped` status are
  reserved for this.
- **Phase 3 — codespace as a git remote.** Run `git http-backend` inside the
  sprite so you can push/pull to the codespace *itself*, not just GitHub.
- **Phase 4 — build off a codespace.** Let a build target a codespace's working
  tree instead of a fresh clone.

## Build target repos

A target repo just needs a **`Dockerfile`** at `dockerfile_path` (default
`Dockerfile`) that produces a container listening on `container_port`. A typical
Rust + React app uses a multi-stage Dockerfile (build the SPA, build the Rust
binary, serve both) — exactly like this repo's own `Dockerfile`.

## Deploying (Railway)

Two services off the same repo:

- **API server** — root `railway.toml` / root `Dockerfile`. Serves the API and
  the built SPA from `/app/static`. Healthcheck `/api/health`.
- **Build worker** — `worker/railway.toml` / `worker/Dockerfile`
  (set the service root directory to the repo root).

Each service gets its own variables (Railway does not share them across
services — use reference variables to keep them in sync):

- **Worker** needs only `DATABASE_URL`, `SPRITES_TOKEN`, `SPRITES_ORG` (plus
  optional `WORKER_POLL_SECS` / `DB_MAX_CONNECTIONS`). It does **not** need the
  GitHub OAuth vars. Example references:
  `DATABASE_URL = ${{Postgres.DATABASE_URL}}`,
  `SPRITES_TOKEN = ${{<server-service>.SPRITES_TOKEN}}`,
  `SPRITES_ORG = ${{<server-service>.SPRITES_ORG}}`.
- **Server** additionally needs `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`,
  `BACKEND_URL`, and `FRONTEND_URL`. In production set `BACKEND_URL` /
  `FRONTEND_URL` to your real `https://` origin and update the GitHub OAuth
  callback URL to match.

## Notes & assumptions

- The sprites REST shapes are coded against the public docs
  (`https://docs.sprites.dev/api/`): `POST /v1/sprites`, `POST
  /v1/sprites/{name}/exec`, public URL `https://<sprite>-<org>.sprites.dev`. If
  the API rev changes (`set_url_public`, exec response framing), adjust
  `backend/src/sprites.rs` — it's isolated there.
- Docker-in-sprite: the build script installs Docker if it's missing and starts
  `dockerd`. Depending on your sprite base image and network egress policy you
  may want to pre-bake Docker into a sprite checkpoint instead.
- The GitHub token is used via a git credential helper (never placed in the clone
  URL) and is redacted from stored logs. It is, however, still stored **plaintext
  at rest** in `users.github_token` — encrypt it (and prefer a GitHub App /
  short-lived tokens) before multi-tenant use.
- Migrations run automatically on startup of both the server and the worker;
  sqlx takes a Postgres advisory lock so concurrent startup is safe. You can also
  run them explicitly as a deploy step: `cargo run --bin migrate`.
```

## ADR compliance

This repo follows the [Architectural Design Records](https://github.com/ethereumdegen/architectural-design-spec).
Highlights and how each is mechanically enforced live in
[`enforcement/`](enforcement/README.md):

- **0001** library crate + thin server/worker/migrate bins
- **0002 / 0015** SQLx with **compile-checked** `query!`/`query_as!` macros; ORMs
  banned via `cargo deny`. The committed `backend/.sqlx/` cache + `SQLX_OFFLINE=true`
  let the Docker build compile without a DB.
- **0003** owner/tenant scoping on all data access · **0004 / 0016** capability-based
  authz via typed `AuthUser` / `AdminUser` extractors
- **0005** config from env, fail-fast on malformed values
- **0006** Postgres job queue with `FOR UPDATE SKIP LOCKED`, no broker
- **0007** per-domain Zustand stores · **0008** one typed API client
- **0010** no panics on request paths (boot-only carve-outs) · **0012** errors map
  to correct HTTP status as `{ "error": … }` · **0013** structured logging only

Run the checks: `./enforcement/adr-checks.sh`,
`(cd backend && cargo clippy --all-targets -- -D warnings)`.
