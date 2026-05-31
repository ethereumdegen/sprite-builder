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
│   │   ├── auth.rs           # GitHub OAuth, sessions, API-key bearer extractor
│   │   ├── github.rs         # GitHub REST client
│   │   ├── sprites.rs        # sprites.dev REST client
│   │   ├── projects.rs       # projects / repos / builds routes
│   │   ├── models.rs         # DB models
│   │   ├── config.rs · error.rs
│   │   └── migrations/0001_init.sql
├── frontend/                 # React + Vite SPA
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
| GET/POST | `/api/keys`                     | list / create API keys              |
| DELETE | `/api/keys/:id`                   | revoke an API key                    |

## How a build runs (the worker)

For each `queued` build the worker:

1. claims the row atomically (`FOR UPDATE SKIP LOCKED`),
2. creates a sprite (`POST /v1/sprites`),
3. runs a build script via `POST /v1/sprites/{name}/exec` that:
   `git clone` the repo at the target commit (using the owner's GitHub token) →
   `docker build` the `Dockerfile` → `docker run -d -p 8080:<container_port>`,
4. makes the sprite URL public and writes
   `status`, `url`, `logs`, and `metadata` back to the build row.

**sprites.dev proxies external traffic to port `8080` inside the VM**, so the
worker maps the container's port to host `8080`. Set a project's `container_port`
to whatever your app listens on (default `8080`).

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

Both need the same env vars (`DATABASE_URL`, `GITHUB_*`, `SPRITES_*`,
`SPRITES_ORG`). In production set `BACKEND_URL` and `FRONTEND_URL` to your real
`https://` origin and update the GitHub OAuth callback URL to match.

## Notes & assumptions

- The sprites REST shapes are coded against the public docs
  (`https://docs.sprites.dev/api/`): `POST /v1/sprites`, `POST
  /v1/sprites/{name}/exec`, public URL `https://<sprite>-<org>.sprites.dev`. If
  the API rev changes (`set_url_public`, exec response framing), adjust
  `backend/src/sprites.rs` — it's isolated there.
- Docker-in-sprite: the build script installs Docker if it's missing and starts
  `dockerd`. Depending on your sprite base image and network egress policy you
  may want to pre-bake Docker into a sprite checkpoint instead.
- The GitHub token is embedded into the clone URL inside the exec script (over
  HTTPS to the sprites API). Fine for a single-tenant tool; for multi-tenant use,
  prefer short-lived tokens / a git credential helper and encrypt tokens at rest.
```
