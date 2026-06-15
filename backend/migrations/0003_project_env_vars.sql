-- Per-project environment variables, injected into the deployed container's
-- `docker run` (via --env-file) by the build worker. Values are returned to the
-- owning user (reveal in the UI) but redacted from build/runtime logs.
CREATE TABLE IF NOT EXISTS project_env_vars (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, key)
);

CREATE INDEX IF NOT EXISTS idx_project_env_vars_project ON project_env_vars(project_id);
