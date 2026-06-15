// Typed client for the sprite-builder backend. All requests use the session
// cookie (same-origin via the Vite proxy in dev). This is the single point of
// contact with the backend (ADR 0008) — components never call fetch directly.

export type Capability = "view_admin_dashboard" | "manage_users";

export interface User {
  id: string;
  github_login: string;
  name: string | null;
  avatar_url: string | null;
  role: "user" | "admin";
  capabilities: Capability[];
}

export interface Repo {
  id: number;
  full_name: string;
  name: string;
  private: boolean;
  default_branch: string;
  description: string | null;
  html_url: string;
  updated_at: string | null;
}

export interface Project {
  id: string;
  name: string;
  repo_full_name: string;
  repo_id: number | null;
  default_branch: string;
  dockerfile_path: string;
  container_port: number;
  created_at: string;
}

export type BuildStatus = "queued" | "running" | "succeeded" | "failed";

export interface Build {
  id: string;
  project_id: string;
  commit_sha: string;
  status: BuildStatus;
  sprite_name: string | null;
  url: string | null;
  logs: string;
  error: string | null;
  metadata: Record<string, unknown>;
  created_at: string;
  updated_at: string;
  started_at: string | null;
  finished_at: string | null;
}

export interface ApiKey {
  id: string;
  name: string;
  key_prefix: string;
  last_used_at: string | null;
  created_at: string;
}

// --- admin dashboard ---

export interface AdminStats {
  users: number;
  projects: number;
  builds_total: number;
  builds_queued: number;
  builds_running: number;
  builds_succeeded: number;
  builds_failed: number;
}

export interface AdminBuild {
  id: string;
  status: BuildStatus;
  commit_sha: string;
  sprite_name: string | null;
  url: string | null;
  error: string | null;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  project_id: string;
  project_name: string;
  repo_full_name: string;
  owner_login: string;
}

export interface AdminUser {
  id: string;
  github_login: string;
  name: string | null;
  role: "user" | "admin";
  created_at: string;
  projects: number;
  builds: number;
}

// A live sprites.dev VM, joined to the build that provisioned it (if known).
// `orphaned` means no build references it anymore — a reclaim candidate.
export interface AdminSprite {
  name: string;
  status: string | null;
  created_at: string | null;
  public_url: string;
  orphaned: boolean;
  build_id: string | null;
  build_status: BuildStatus | null;
  project_id: string | null;
  project_name: string | null;
  owner_login: string | null;
}

class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function req<T>(path: string, options: RequestInit = {}): Promise<T> {
  const res = await fetch(path, {
    credentials: "include",
    headers: { "Content-Type": "application/json", ...(options.headers || {}) },
    ...options,
  });
  if (!res.ok) {
    let msg = res.statusText;
    try {
      const body = await res.json();
      if (body?.error) msg = body.error;
    } catch {
      /* ignore */
    }
    throw new ApiError(res.status, msg);
  }
  if (res.status === 204) return undefined as T;
  return res.json() as Promise<T>;
}

export const api = {
  loginUrl: "/api/auth/github",
  me: () => req<User>("/api/me"),
  logout: () => req<{ ok: boolean }>("/api/auth/logout", { method: "POST" }),

  repos: () => req<Repo[]>("/api/repos"),

  projects: () => req<Project[]>("/api/projects"),
  project: (id: string) => req<Project>(`/api/projects/${id}`),
  createProject: (body: {
    name: string;
    repo_full_name: string;
    repo_id?: number;
    default_branch?: string;
    dockerfile_path?: string;
    container_port?: number;
  }) => req<Project>("/api/projects", { method: "POST", body: JSON.stringify(body) }),

  builds: (projectId: string) => req<Build[]>(`/api/projects/${projectId}/builds`),
  build: (id: string) => req<Build>(`/api/builds/${id}`),
  createBuild: (projectId: string, commit_sha?: string) =>
    req<Build>(`/api/projects/${projectId}/builds`, {
      method: "POST",
      body: JSON.stringify({ commit_sha }),
    }),

  keys: () => req<ApiKey[]>("/api/keys"),
  createKey: (name: string) =>
    req<{ key: ApiKey; secret: string }>("/api/keys", {
      method: "POST",
      body: JSON.stringify({ name }),
    }),
  deleteKey: (id: string) => req<{ ok: boolean }>(`/api/keys/${id}`, { method: "DELETE" }),

  // admin
  adminStats: () => req<AdminStats>("/api/admin/stats"),
  adminBuilds: (status?: string) =>
    req<AdminBuild[]>(
      `/api/admin/builds${status ? `?status=${encodeURIComponent(status)}` : ""}`
    ),
  adminRebuild: (buildId: string) =>
    req<AdminBuild>(`/api/admin/builds/${buildId}/rebuild`, { method: "POST" }),
  adminSprites: () => req<AdminSprite[]>("/api/admin/sprites"),
  adminDeleteSprite: (name: string) =>
    req<void>(`/api/admin/sprites/${encodeURIComponent(name)}`, { method: "DELETE" }),
  adminSetSpritePublic: (name: string) =>
    req<void>(`/api/admin/sprites/${encodeURIComponent(name)}/public`, { method: "POST" }),
  adminUsers: () => req<AdminUser[]>("/api/admin/users"),
  adminSetRole: (id: string, role: "user" | "admin") =>
    req<AdminUser>(`/api/admin/users/${id}/role`, {
      method: "PATCH",
      body: JSON.stringify({ role }),
    }),
};

export { ApiError };
