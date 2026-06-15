-- User roles for capability-based authorization (ADR 0016).
-- Role -> capability mapping lives in code (backend/src/authz.rs); the column
-- only stores the coarse role. New users default to the least-privileged role.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'user';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'users_role_chk'
    ) THEN
        ALTER TABLE users
            ADD CONSTRAINT users_role_chk CHECK (role IN ('user', 'admin'));
    END IF;
END$$;

CREATE INDEX IF NOT EXISTS idx_users_role ON users(role);
