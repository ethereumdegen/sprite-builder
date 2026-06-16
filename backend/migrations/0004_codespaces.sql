-- Codespaces: an ephemeral coding filesystem (a long-lived sprite holding a git
-- working tree), owned by a project. Decoupled from the builds system.

-- status: queued | provisioning | ready | stopped | failed
--   queued       -> just created, waiting for the worker
--   provisioning -> worker is creating the sprite + cloning the repo
--   ready        -> sprite is live, /workspace/app holds the clone
--   stopped      -> reserved for phase 2 (S3 hibernation); unused in v1
--   failed       -> provisioning failed (see error)
CREATE TABLE IF NOT EXISTS codespaces (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    branch       TEXT NOT NULL,            -- branch checked out in the worktree
    status       TEXT NOT NULL DEFAULT 'queued',
    sprite_name  TEXT,                     -- live sprite (NULL when stopped/hibernated)
    url          TEXT,                     -- sprite public URL (later: git-remote / preview)
    snapshot_key TEXT,                     -- S3 object key when hibernated (phase 2, unused in v1)
    logs         TEXT NOT NULL DEFAULT '', -- provisioning/clone log
    error        TEXT,
    metadata     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at   TIMESTAMPTZ,              -- when the worker claimed it (provisioning start)
    finished_at  TIMESTAMPTZ               -- when it reached ready/failed
);
CREATE INDEX IF NOT EXISTS idx_codespaces_project ON codespaces(project_id);
CREATE INDEX IF NOT EXISTS idx_codespaces_status  ON codespaces(status);
