-- Initial schema for sprite-builder.

CREATE TABLE IF NOT EXISTS users (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    github_id          BIGINT NOT NULL UNIQUE,
    github_login       TEXT NOT NULL,
    name               TEXT,
    avatar_url         TEXT,
    -- GitHub OAuth access token, used server-side to list repos / clone.
    github_token       TEXT NOT NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Browser sessions (cookie-based) for the web UI.
CREATE TABLE IF NOT EXISTS sessions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    token       TEXT NOT NULL UNIQUE,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at  TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);

-- Bearer API keys for programmatic API access. Only the SHA-256 hash is stored.
CREATE TABLE IF NOT EXISTS api_keys (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    key_hash     TEXT NOT NULL UNIQUE,
    key_prefix   TEXT NOT NULL,          -- first chars, shown in UI for identification
    last_used_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);

CREATE TABLE IF NOT EXISTS projects (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    repo_full_name  TEXT NOT NULL,        -- "owner/repo"
    repo_id         BIGINT,
    default_branch  TEXT NOT NULL DEFAULT 'main',
    dockerfile_path TEXT NOT NULL DEFAULT 'Dockerfile',
    container_port  INT NOT NULL DEFAULT 8080,  -- port the app listens on inside the container
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_projects_user ON projects(user_id);

-- build status: queued | running | succeeded | failed
CREATE TABLE IF NOT EXISTS builds (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    commit_sha   TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'queued',
    sprite_name  TEXT,
    url          TEXT,
    logs         TEXT NOT NULL DEFAULT '',
    error        TEXT,
    metadata     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at   TIMESTAMPTZ,
    finished_at  TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_builds_project ON builds(project_id);
CREATE INDEX IF NOT EXISTS idx_builds_status ON builds(status);
