# Codespaces — an ephemeral coding filesystem subapp

A new subapp within sprite-builder, decoupled from the sprites/builds system. A
**Codespace** is a long-lived sprite owned by a project, holding a git clone of
the project's repo in `/workspace/app`. Interactive file/bash/git operations run
synchronously through the API server; provisioning runs through the worker.
Pushes/pulls go to GitHub (the third-party remote).

```
project ──▶ create Codespace ──▶ [worker] provision sprite + git clone /workspace/app ──▶ status=ready
                                                  │
   ┌──────────────────────────────────────────────┘
   │  interactive (API server, synchronous, bounded exec — like runtime_logs):
   ├─ list/read/write/delete files under /workspace/app  (path-jailed, secret-redacted)
   ├─ run arbitrary bash                                  ("as if on the local machine")
   └─ git status / diff / commit / push / pull            (token via credential helper)
                                                  │
   (phase 2) stop ──▶ [worker] tar /workspace ──▶ S3/DO Spaces ──▶ delete sprite  (true "ephemeral")
             start ──▶ [worker] new sprite ──▶ restore tarball from S3
```

## Decisions (locked)

- **Persistence:** sprite stays alive as the live filesystem in v1; S3/DO Spaces
  hibernation (snapshot + stop/start) is Phase 2.
- **Git remote:** v1 clones from and pushes/pulls to **GitHub** only. Making the
  codespace itself a pushable git remote (in-sprite `git http-backend`) is Phase 3.
- **Commit author:** owner's GitHub login + `<login>@users.noreply.github.com`.
- **Branch model:** always clone the project's `default_branch` in v1 (the
  `branch` column is still populated for forward-compat and because push/pull
  need it). User can branch manually via the exec/git API.
- **Exec in UI:** the `exec` endpoint ships and is live, but there is no terminal
  box in the v1 UI (API-only). v1 UI is files + commit/push.

## Phase 1 — implementation

### 1. Migration `backend/migrations/0004_codespaces.sql`

```sql
CREATE TABLE codespaces (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    branch       TEXT NOT NULL,            -- branch checked out in the worktree
    status       TEXT NOT NULL DEFAULT 'queued',  -- queued|provisioning|ready|stopped|failed
    sprite_name  TEXT,                     -- live sprite (NULL when stopped/hibernated)
    url          TEXT,                     -- sprite public URL (later: git-remote / preview)
    snapshot_key TEXT,                     -- S3 object key when hibernated (phase 2, unused in v1)
    logs         TEXT NOT NULL DEFAULT '', -- provisioning/clone log
    error        TEXT,
    metadata     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_codespaces_project ON codespaces(project_id);
CREATE INDEX idx_codespaces_status  ON codespaces(status);
```

Status lifecycle (v1): `queued → provisioning → ready` (`failed` on error).
`stopped` is reserved for Phase 2. Ownership reached through the parent project
(ADR-0003).

### 2. `backend/src/models.rs` — `Codespace` struct

`FromRow + Serialize`, mirrors `Build`. Project-scoped; ownership via the project.

### 3. `backend/src/codespaces.rs` — new domain module

Thin handlers (ADR-0001), `AuthUser` extractor + a `load_owned_codespace` helper
mirroring `load_owned_build` (ADR-0003/0004).

| Method | Path | Notes |
|---|---|---|
| GET/POST | `/api/projects/:id/codespaces` | list / create (insert `queued`, return row) |
| GET | `/api/codespaces/:id` | status + metadata (UI polls while `provisioning`) |
| DELETE | `/api/codespaces/:id` | delete sprite (best-effort) + row |
| GET | `/api/codespaces/:id/files?path=` | list dir or read file → `{content, truncated, binary}` |
| PUT | `/api/codespaces/:id/files` | `{path, content}`, base64-shipped, size-capped |
| DELETE | `/api/codespaces/:id/files?path=` | delete path |
| POST | `/api/codespaces/:id/exec` | `{cmd}` → `{output, exit_code}` (live; no UI in v1) |
| POST | `/api/codespaces/:id/git` | `{op: status\|diff\|commit\|push\|pull, message?}` |

Every interactive handler: bounded `tokio::time::timeout` + `app.sprites.exec`,
secrets (GitHub token + project env values) redacted from output (ADR-0013). A
**path-jail helper** rejects absolute paths / `..` and contains every files op
under `/workspace/app`.

### 4. Worker — codespace provisioning loop

A second loop spawned in `worker::run`, independent of the build loop, claiming
`queued` codespaces with `FOR UPDATE SKIP LOCKED` (ADR-0006). It:

1. creates a sprite (unique name vs `codespaces.sprite_name`),
2. `git clone` repo @ `default_branch` into `/workspace/app` via the credential
   helper, streaming clone output into `codespaces.logs` (same live-log pattern
   as builds),
3. sets `git config user.name = <github_login>`,
   `user.email = <login>@users.noreply.github.com` in the worktree,
4. marks `ready`.

No Docker / `docker run` — that's the only difference from a build. A sibling
reaper fails codespaces stuck in `provisioning` too long.

### 5. `backend/src/lib.rs`

`pub mod codespaces;` + mount the routes. No `sprites.rs` or `config.rs` changes
in v1.

### 6. Frontend

`stores/codespaces.ts` (ADR-0007) + methods on the single `api.ts` client
(ADR-0008). A **Codespaces** tab on the project page (list + "New codespace" →
clones default branch). Detail page: polls status while provisioning, then a
file tree + textarea editor (read/write via the files API) + a **Commit & Push**
panel (the git endpoint). No terminal box in v1 (exec is API-only).

## ADR compliance

Thin handlers in a new domain module (0001); `AuthUser` extractor + project-scoped
ownership (0003/0004); Postgres queue with `FOR UPDATE SKIP LOCKED`, no broker
(0006); env config fail-fast (0005); secret redaction + structured logs (0013);
`{ "error": … }` contract via `AppError` (0012); per-domain Zustand store + single
typed client (0007/0008). Matches the existing runtime-checked `query_as::<_, T>`
SQL style used throughout `projects.rs` (consistent with ADR-0017).

## Deferred phases

- **Phase 2 (ephemerality):** S3/DO Spaces snapshot + stop/start hibernation,
  presigned-URL based (S3 creds stay server-side, ADR-0011).
- **Phase 3 (git remote):** codespace-as-git-remote via in-sprite `git http-backend`.
- **Phase 4 (later):** let a build target a Codespace instead of a fresh clone.

## To verify during implementation

Sprite idle lifecycle — whether sprites.dev auto-suspends/tears down an idle
sprite. Affects whether a `ready` codespace stays reachable between edits; if they
auto-suspend, it strengthens the case for Phase 2 hibernation.
