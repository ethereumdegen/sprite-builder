"use strict";

// Thin SDK over the sprite-builder HTTP API. One method per endpoint, plus a
// couple of conveniences (allBuilds across projects, build-id prefix resolution)
// the CLI leans on. Uses the global `fetch` available in Node 18+.

const DEFAULT_BASE_URL = "https://sprite-builder-production.up.railway.app";

class ApiError extends Error {
  constructor(status, message) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

class SpriteBuilder {
  constructor({ baseUrl, token } = {}) {
    this.baseUrl = (baseUrl || DEFAULT_BASE_URL).replace(/\/+$/, "");
    this.token = token;
  }

  async req(path, { method = "GET", body } = {}) {
    if (!this.token) {
      throw new ApiError(0, "no API token — set SPRITE_BUILDER_TOKEN or pass --token");
    }
    let res;
    try {
      res = await fetch(`${this.baseUrl}${path}`, {
        method,
        headers: {
          Authorization: `Bearer ${this.token}`,
          ...(body ? { "Content-Type": "application/json" } : {}),
        },
        body: body ? JSON.stringify(body) : undefined,
      });
    } catch (e) {
      throw new ApiError(0, `network error reaching ${this.baseUrl}: ${e.message}`);
    }
    if (!res.ok) {
      let msg = res.statusText;
      try {
        const j = await res.json();
        if (j && j.error) msg = j.error;
      } catch {
        /* non-JSON error body */
      }
      throw new ApiError(res.status, msg);
    }
    if (res.status === 204) return undefined;
    const text = await res.text();
    return text ? JSON.parse(text) : undefined;
  }

  // --- endpoints ---
  me() {
    return this.req("/api/me");
  }
  projects() {
    return this.req("/api/projects");
  }
  builds(projectId) {
    return this.req(`/api/projects/${projectId}/builds`);
  }
  build(id) {
    return this.req(`/api/builds/${id}`);
  }
  runtimeLogs(id) {
    return this.req(`/api/builds/${id}/runtime-logs`);
  }
  urlVisibility(id) {
    return this.req(`/api/builds/${id}/url-visibility`);
  }
  setUrlVisibility(id, isPublic) {
    return this.req(`/api/builds/${id}/url-visibility`, {
      method: "POST",
      body: { public: isPublic },
    });
  }
  envVars(projectId) {
    return this.req(`/api/projects/${projectId}/env`);
  }
  setEnvVar(projectId, key, value) {
    return this.req(`/api/projects/${projectId}/env`, {
      method: "POST",
      body: { key, value },
    });
  }
  deleteEnvVar(projectId, key) {
    return this.req(`/api/projects/${projectId}/env/${encodeURIComponent(key)}`, {
      method: "DELETE",
    });
  }

  // --- conveniences ---

  // Every build the caller owns, newest first, each tagged with its project.
  async allBuilds() {
    const projects = await this.projects();
    const out = [];
    for (const p of projects) {
      const builds = await this.builds(p.id);
      for (const b of builds) out.push({ ...b, project_name: p.name, project_id: p.id });
    }
    out.sort((a, b) => (a.created_at < b.created_at ? 1 : -1));
    return out;
  }

  // Resolve a full or prefix build id to a single build. Throws on no/ambiguous match.
  async resolveBuild(idOrPrefix) {
    const all = await this.allBuilds();
    const exact = all.find((b) => b.id === idOrPrefix);
    if (exact) return exact;
    const matches = all.filter((b) => b.id.startsWith(idOrPrefix));
    if (matches.length === 1) return matches[0];
    if (matches.length === 0) throw new ApiError(404, `no build matching "${idOrPrefix}"`);
    throw new ApiError(
      400,
      `"${idOrPrefix}" is ambiguous (${matches.length} matches: ${matches
        .map((b) => b.id.slice(0, 8))
        .join(", ")})`
    );
  }
}

module.exports = { SpriteBuilder, ApiError, DEFAULT_BASE_URL };
