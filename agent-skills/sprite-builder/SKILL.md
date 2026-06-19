---
name: sprite-builder
description: >
  Drive the Sprite Builder HTTP API. Sprite Builder has THREE facets, all hung off
  a Project (a GitHub repo): (1) Builds — Docker-build a repo in a sprites.dev
  sandbox and get a live URL; (2) Codespaces — a long-lived sprite holding a git
  working tree you can read/write/exec/git against; (3) Docuspaces — an S3-backed
  file store with no sprite. Use when the user wants to deploy/build a repo,
  spin up or operate a dev sandbox, manage S3-backed project files, or manage
  projects/env-vars/API keys programmatically.
---

# Sprite Builder API

Sprite Builder is built around a **Project** (a GitHub repo + settings). A project
has **three independent facets**, each its own resource type:

| Facet | What it is | Backed by | Lifecycle |
|-------|-----------|-----------|-----------|
| **Builds** | Build the repo's Docker image at a commit, run it, return a public URL | a fresh sprites.dev sandbox per build | `queued → running → succeeded \| failed` |
| **Codespaces** | A long-lived sprite holding a git working tree at `/workspace/app`; read/write files, run bash, run git | one persistent sprites.dev sprite | `queued → provisioning → ready \| failed` |
| **Docuspaces** | An S3-backed file store (markdown/assets); no sprite, no worker | S3 objects under `docuspaces/<id>/…` | instant (record-only) |

Pick the facet by intent: **ship a live URL → Builds. Interactive coding/exec in a
sandbox → Codespaces. Just store/serve files → Docuspaces.** State lives in Postgres
(Builds/Codespaces metadata) and S3 (Docuspace file bytes).

## Auth & configuration

Every `/api/*` route accepts a bearer **API key** (or a web session cookie, which
you won't have). Get a key from the **API Keys** page in the UI — the secret
(`sb_…`) is shown **once**; only its SHA-256 hash is stored, so it can't be
recovered later (delete + recreate if lost).

Read both values from the environment — **never hardcode the key** or commit it:

```bash
: "${SPRITE_BUILDER_API_KEY:?set SPRITE_BUILDER_API_KEY to your sb_… API key}"
: "${SPRITE_BUILDER_BASE_URL:=http://localhost:5173}"   # or your deployed https origin
```

All requests:

```bash
curl -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" "$SPRITE_BUILDER_BASE_URL/api/..."
```

Quick check: `GET /api/health` → `{"ok":true}` (no auth). `GET /api/me` confirms
the key is valid and returns the owning user.

## Facet 1 — Builds (deploy a repo to a live URL)

A **Project** is a repo + build settings. A **Build** is one attempt that
produces a deployed URL. Builds belong to projects.

### 1. Create a project

```bash
curl -X POST -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"name":"my-app","repo_full_name":"me/my-app","default_branch":"main","container_port":8080}' \
  "$SPRITE_BUILDER_BASE_URL/api/projects"
```

`CreateProjectBody` fields:

| field | required | default | notes |
|-------|:--------:|---------|-------|
| `name` | ✅ | — | display name |
| `repo_full_name` | ✅ | — | `owner/repo`; must be reachable by the key owner's GitHub token |
| `repo_id` | | null | GitHub numeric repo id (optional) |
| `default_branch` | | `main` | branch whose HEAD builds when no commit given |
| `dockerfile_path` | | `Dockerfile` | path within the repo |
| `container_port` | | `8080` | port the container listens on; mapped to the public URL |

Returns the `Project` (note its `id` — a UUID).

### 2. Trigger a build

```bash
# HEAD of the project's default branch (body optional — {} or omitted both work)
curl -X POST -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' -d '{}' \
  "$SPRITE_BUILDER_BASE_URL/api/projects/<project-id>/builds"

# pin a specific commit
curl -X POST -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"commit_sha":"<sha>"}' \
  "$SPRITE_BUILDER_BASE_URL/api/projects/<project-id>/builds"
```

`commit_sha` is optional; omitted ⇒ the worker resolves HEAD of `default_branch`
via GitHub. Returns a `Build` in status `queued`.

### 3. Poll the build

```bash
curl -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  "$SPRITE_BUILDER_BASE_URL/api/builds/<build-id>"
```

A background worker (polls every few seconds) does the work. **Status lifecycle:**

```
queued ──▶ running ──▶ succeeded   (build.url is set, container is live)
                   └──▶ failed      (build.error / build.logs explain why)
```

Poll until `status` is `succeeded` or `failed`. On success, `url` holds the live
URL. `logs` streams build output; `error` is set on failure. Builds can take a
few minutes (first Rust/Docker build especially).

A minimal poll loop:

```bash
while :; do
  b=$(curl -s -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
        "$SPRITE_BUILDER_BASE_URL/api/builds/$BUILD_ID")
  st=$(printf '%s' "$b" | jq -r .status)
  echo "status=$st"
  case "$st" in
    succeeded) printf '%s' "$b" | jq -r .url; break ;;
    failed)    printf '%s' "$b" | jq -r .error; break ;;
  esac
  sleep 5
done
```

### Build object fields

`id, project_id, commit_sha, status, sprite_name, url, logs, error, metadata,
created_at, updated_at, started_at, finished_at`.

## Environment variables (injected into the deployed container)

Env vars are per-project and injected at `docker run` (runtime, not build args).
Values are redacted from persisted logs.

```bash
# list
curl -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  "$SPRITE_BUILDER_BASE_URL/api/projects/<project-id>/env"

# set / update (upsert)
curl -X POST -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"key":"DATABASE_URL","value":"postgres://…"}' \
  "$SPRITE_BUILDER_BASE_URL/api/projects/<project-id>/env"

# delete one
curl -X DELETE -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  "$SPRITE_BUILDER_BASE_URL/api/projects/<project-id>/env/<KEY>"
```

Key names: letters/digits/underscores, not starting with a digit. Set env vars
**before** triggering the build you want them in.

## Other build endpoints

```bash
# list a project's builds
GET  /api/projects/:id/builds
# get the project
GET  /api/projects/:id
# list your projects
GET  /api/projects
# list repos the key owner can access (from GitHub)
GET  /api/repos
# runtime logs from the live container
GET  /api/builds/:id/runtime-logs
# read / toggle public-vs-private access on a deployed build's URL
GET  /api/builds/:id/url-visibility
POST /api/builds/:id/url-visibility   body: {"public": true}
```

## Facet 2 — Codespaces (long-lived dev sandbox)

A **Codespace** is a persistent sprites.dev sprite holding a git working tree at
`/workspace/app`. Use it for interactive coding: read/write files, run arbitrary
bash, and run git — "as if on a local machine." Project-scoped; provisioned
asynchronously by the worker.

### Lifecycle

```
queued ──▶ provisioning ──▶ ready     (sprite is live; file/exec/git ops work)
                       └──▶ failed     (see codespace.error / .logs)
```

**All file/exec/git/clone ops require `status == "ready"`** — otherwise they
`400` with `"codespace is not ready (status: …)"`. So: create → poll
`GET /api/codespaces/:id` until `ready` → then operate.

### Create, list, get, rename, delete

```bash
# create (queues provisioning; body optional — {} fine). Branch = project default.
curl -X POST -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' -d '{"name":"scratch"}' \
  "$SPRITE_BUILDER_BASE_URL/api/projects/<project-id>/codespaces"

GET    /api/projects/:id/codespaces     # list a project's codespaces
GET    /api/codespaces/:id              # get one (poll its status here)
PATCH  /api/codespaces/:id              # rename — body {"name":"…"} (label only)
DELETE /api/codespaces/:id              # tear down the sprite
```

`CreateCodespaceBody`: `{ "name"?: string }` (defaults to a random adjective-noun).
The `Codespace` object: `id, project_id, name, branch, status, sprite_name, url,
snapshot_key, logs, error, metadata, created_at, updated_at, started_at,
finished_at`.

### Clone a repo into the sandbox

Provisioning no longer auto-clones — clone explicitly. Uses the owner's GitHub
token (via a credential helper, never in the URL) and **replaces** `/workspace/app`.

```bash
curl -X POST -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' -d '{}' \
  "$SPRITE_BUILDER_BASE_URL/api/codespaces/<id>/clone"
```

`CloneBody`: `{ "repo_full_name"?: "owner/repo", "branch"?: "main" }` — both default
to the codespace's project repo + branch.

### Files: read / list, write, delete

```bash
# read a file OR list a directory (path empty/omitted = workspace root)
curl -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  "$SPRITE_BUILDER_BASE_URL/api/codespaces/<id>/files?path=src/main.rs"

# write (create/overwrite) a file
curl -X PUT -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"path":"src/main.rs","content":"fn main(){}"}' \
  "$SPRITE_BUILDER_BASE_URL/api/codespaces/<id>/files"

# delete a file or dir
curl -X DELETE -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  "$SPRITE_BUILDER_BASE_URL/api/codespaces/<id>/files?path=tmp/junk"
```

- Paths are **relative to and jailed under `/workspace/app`** — `..` escapes and
  absolute paths are rejected; you cannot write/delete the workspace root itself.
- `WriteFileBody`: `{ "path": string, "content": string }`. Max file **1 MiB**.
- Read returns `ReadResult`: `{ kind: "file"|"dir", path, entries?: [{name,is_dir}],
  content?, binary, truncated, size }`. Binary files come back base64 with
  `binary:true`.

### Exec arbitrary bash

```bash
curl -X POST -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' -d '{"cmd":"cargo test 2>&1 | tail -20"}' \
  "$SPRITE_BUILDER_BASE_URL/api/codespaces/<id>/exec"
```

`ExecBody`: `{ "cmd": string }`. Runs from `/workspace/app`. Returns
`{ "output": string, "exit_code": int }` (merged stdout+stderr; `exit_code` is
`-1` if the channel dropped). Project env-var values are **redacted** from output.

### Git

```bash
curl -X POST -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' -d '{"op":"commit","message":"wip"}' \
  "$SPRITE_BUILDER_BASE_URL/api/codespaces/<id>/git"
```

`GitBody`: `{ "op": "status"|"diff"|"commit"|"push"|"pull", "message"?: string }`
(`message` required for `commit`). `push`/`pull` authenticate with the owner's
GitHub token. Returns `{ op, output, exit_code }`.

## Facet 3 — Docuspaces (S3-backed file store, no sprite)

A **Docuspace** is plain S3-backed file storage owned by a project — **no sprite,
no worker, no provisioning** (creation is instant). Use it to store/serve files
(markdown, images, assets) without a running sandbox. Folders are **implicit**: a
folder exists when something lives under `<path>/`.

> Requires the server's `S3_*` env (`S3_ENDPOINT`, `S3_ACCESS_KEY`,
> `S3_SECRET_KEY`, …) to be configured. If not, file ops return a clean
> `400 "S3 is not configured"` — the docuspace record still works, but reads/writes won't.

### Create, list, get, rename, delete

```bash
# create (instant — no sprite)
curl -X POST -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' -d '{"name":"docs"}' \
  "$SPRITE_BUILDER_BASE_URL/api/projects/<project-id>/docuspaces"

GET    /api/projects/:id/docuspaces     # list
GET    /api/docuspaces/:id              # get
PATCH  /api/docuspaces/:id              # rename — body {"name":"…"}
DELETE /api/docuspaces/:id              # delete (drops all its objects)
```

`CreateDocuspaceBody`: `{ "name"?: string }`. The `Docuspace` object:
`id, project_id, name, metadata, created_at, updated_at`.

### Files & folders

```bash
# read a file OR list a directory (path empty/omitted = root)
curl -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  "$SPRITE_BUILDER_BASE_URL/api/docuspaces/<id>/files?path=README.md"

# write (create/overwrite). encoding "utf8" (default) or "base64" for binaries.
curl -X PUT -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"path":"notes/readme.md","content":"# Hi","encoding":"utf8"}' \
  "$SPRITE_BUILDER_BASE_URL/api/docuspaces/<id>/files"

# delete a file, or a folder and everything under it
curl -X DELETE -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  "$SPRITE_BUILDER_BASE_URL/api/docuspaces/<id>/files?path=notes/readme.md"

# create an (empty) folder — only needed to make a folder visible before it has files
curl -X POST -H "Authorization: Bearer $SPRITE_BUILDER_API_KEY" \
  -H 'Content-Type: application/json' -d '{"path":"images"}' \
  "$SPRITE_BUILDER_BASE_URL/api/docuspaces/<id>/folders"
```

- `WriteFileBody`: `{ "path": string, "content": string, "encoding"?: "utf8"|"base64" }`.
  Max file **5 MiB**. Content-type is inferred from the extension (so markdown/images
  render/download correctly). Use `encoding:"base64"` to upload binary files.
- `CreateFolderBody`: `{ "path": string }` (writes a `.keep` marker).
- Read returns `ReadResult`: `{ kind: "file"|"dir", path, entries?, content?, binary, size }`.
- Paths are validated/jailed (no `..`, no absolute paths), same as codespaces.

## Admin (requires the `admin` role / capability)

Regular API keys won't reach these unless the owner is an admin. App-wide
visibility + management:

```bash
GET    /api/admin/stats
GET    /api/admin/builds                # every build across all users
POST   /api/admin/builds/:id/rebuild
GET    /api/admin/sprites
DELETE /api/admin/sprites/:name
POST   /api/admin/sprites/:name/public
GET    /api/admin/users
PATCH  /api/admin/users/:id/role        # promote/demote
```

## Notes & gotchas

- **All routes are owner-scoped** — a key only sees the projects/builds owned by
  its user (admin endpoints excepted). 404s usually mean "not yours," not "gone."
- **The repo must be reachable by the key owner's GitHub token** (the OAuth grant
  used `read:user repo`). Building someone else's private repo fails at clone.
- **IDs are UUIDs.** `project_id` and `build_id` are distinct — don't cross them.
- **`BASE` is the browser-facing origin**, not the backend's `:8787`. In dev that's
  `http://localhost:5173` (Vite proxies `/api`); in prod it's the public https origin.
- A build's URL is **server-assigned** — read `build.url`, don't construct it.
