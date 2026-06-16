-- Docuspaces: an S3-backed file store (usually markdown), owned by a project.
-- Unlike codespaces there is no sprite and no worker — files live as plain
-- objects under the key prefix `docuspaces/<id>/...` in the configured S3 bucket,
-- so the only state we keep in Postgres is the docuspace record itself.
CREATE TABLE IF NOT EXISTS docuspaces (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    metadata     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_docuspaces_project ON docuspaces(project_id);
