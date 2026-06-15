import { useState } from "react";
import { Link } from "react-router-dom";

// API reference for the sprite-builder backend. Hand-written to match the real
// routes in backend/src/lib.rs; keep in sync when endpoints change.

const base = location.origin;

function Code({ children }: { children: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(children);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* clipboard unavailable */
    }
  };
  return (
    <div className="codewrap">
      <button className="copybtn secondary" onClick={copy}>
        {copied ? "Copied" : "Copy"}
      </button>
      <pre className="code">{children}</pre>
    </div>
  );
}

function Method({ m }: { m: "GET" | "POST" | "DELETE" | "PATCH" }) {
  return <span className={`method ${m.toLowerCase()}`}>{m}</span>;
}

export default function DocsPage() {
  return (
    <div className="docs">
      <h2>API Documentation</h2>
      <p className="muted">
        Sprite Builder turns a GitHub repo into a running deployment: create a
        project, point it at a repo, and trigger a build. The worker clones the
        commit, runs <code>docker build</code> inside a sprite, launches the
        container, and returns a public URL. Everything the web UI does is
        available over this HTTP API.
      </p>

      <div className="card toc">
        <strong>Contents</strong>
        <a href="#auth">Authentication</a>
        <a href="#quickstart">Quickstart: repo → build → URL</a>
        <a href="#keys">API keys</a>
        <a href="#repos">Repositories</a>
        <a href="#projects">Projects</a>
        <a href="#builds">Builds</a>
        <a href="#build-object">The build object</a>
        <a href="#admin">Admin</a>
        <a href="#errors">Errors</a>
      </div>

      {/* ------------------------------------------------------------------ */}
      <h3 id="auth">Authentication</h3>
      <p>
        Every endpoint except <code>/api/health</code> requires
        authentication. Two schemes are accepted:
      </p>
      <ul>
        <li>
          <strong>API key</strong> (programmatic) — send it as a bearer token:
          <br />
          <code>Authorization: Bearer sb_…</code>
        </li>
        <li>
          <strong>Session cookie</strong> (the web UI) — set automatically after
          GitHub login.
        </li>
      </ul>
      <p className="muted">
        A key acts <em>on behalf of the user who created it</em> and inherits
        that user's GitHub access — that's how the API can list your repos and
        resolve the latest commit. Create and revoke keys on the{" "}
        <Link to="/keys">API Keys</Link> page. The full secret is shown exactly
        once at creation; only a hashed prefix is stored afterward.
      </p>
      <p>
        Base URL: <code>{base}</code>
      </p>

      {/* ------------------------------------------------------------------ */}
      <h3 id="quickstart">Quickstart: from repo to live URL</h3>
      <p>
        This is the full flow the question "create a project, link a repo, and
        start a build" maps to. It assumes you've created a key on the{" "}
        <Link to="/keys">API Keys</Link> page and exported it:
      </p>
      <Code>{`export SB_KEY="sb_xxxxxxxxxxxxxxxxxxxxxxxx"
export SB_URL="${base}"`}</Code>

      <h4>1. (Optional) find a repo you can build</h4>
      <p>
        Projects are linked by <code>repo_full_name</code> (e.g.{" "}
        <code>my-org/my-app</code>). List the repos your GitHub account can
        access:
      </p>
      <Code>{`curl -s "$SB_URL/api/repos" \\
  -H "Authorization: Bearer $SB_KEY" | jq '.[].full_name'`}</Code>

      <h4>2. Create a project linked to that repo</h4>
      <p>
        Only <code>name</code> and <code>repo_full_name</code> are required. The
        rest control how the build runs and fall back to sensible defaults
        (branch <code>main</code>, <code>Dockerfile</code>, port{" "}
        <code>8080</code>).
      </p>
      <Code>{`curl -s "$SB_URL/api/projects" \\
  -H "Authorization: Bearer $SB_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "name": "My App",
    "repo_full_name": "my-org/my-app",
    "default_branch": "main",
    "dockerfile_path": "Dockerfile",
    "container_port": 8080
  }'`}</Code>
      <p>
        The response is the new project, including its <code>id</code> (a UUID).
        Capture it:
      </p>
      <Code>{`PROJECT_ID=$(curl -s "$SB_URL/api/projects" \\
  -H "Authorization: Bearer $SB_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"name":"My App","repo_full_name":"my-org/my-app"}' | jq -r .id)`}</Code>

      <h4>3. Start a build</h4>
      <p>
        Trigger a build against the project. With <strong>no body</strong> (or
        an empty <code>commit_sha</code>), the API resolves the latest commit on
        the project's default branch:
      </p>
      <Code>{`# latest commit on the default branch
curl -s "$SB_URL/api/projects/$PROJECT_ID/builds" \\
  -H "Authorization: Bearer $SB_KEY" \\
  -X POST`}</Code>
      <p>To pin a specific commit, pass its SHA:</p>
      <Code>{`# a particular commit
curl -s "$SB_URL/api/projects/$PROJECT_ID/builds" \\
  -H "Authorization: Bearer $SB_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"commit_sha": "a1b2c3d4e5f6..."}'`}</Code>
      <p>
        The response is a build record with <code>status: "queued"</code> and an{" "}
        <code>id</code>. The background worker picks it up within a few seconds.
      </p>

      <h4>4. Poll the build until it finishes</h4>
      <p>
        Builds run asynchronously. Poll <code>GET /api/builds/:id</code> until{" "}
        <code>status</code> is <code>succeeded</code> or <code>failed</code>. On
        success, <code>url</code> is the live deployment.
      </p>
      <Code>{`BUILD_ID=$(curl -s "$SB_URL/api/projects/$PROJECT_ID/builds" \\
  -H "Authorization: Bearer $SB_KEY" -X POST | jq -r .id)

# poll every 5s until the build settles
while :; do
  B=$(curl -s "$SB_URL/api/builds/$BUILD_ID" -H "Authorization: Bearer $SB_KEY")
  STATUS=$(echo "$B" | jq -r .status)
  echo "status: $STATUS"
  case "$STATUS" in
    succeeded) echo "live at: $(echo "$B" | jq -r .url)"; break ;;
    failed)    echo "error: $(echo "$B" | jq -r .error)"; break ;;
  esac
  sleep 5
done`}</Code>

      {/* ------------------------------------------------------------------ */}
      <h3 id="keys">API keys</h3>

      <h4>
        <Method m="POST" /> <span className="mono">/api/keys</span>
      </h4>
      <p>
        Create a key. Returns the key record plus the one-time{" "}
        <code>secret</code> — store it immediately, it can't be retrieved again.
      </p>
      <table>
        <thead>
          <tr>
            <th>Field</th>
            <th>Type</th>
            <th>Notes</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>
              <code>name</code>
            </td>
            <td>string</td>
            <td>Required. A label, e.g. "CI deploy".</td>
          </tr>
        </tbody>
      </table>
      <Code>{`curl -s "$SB_URL/api/keys" \\
  -H "Authorization: Bearer $SB_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"name": "CI deploy"}'
# => { "key": { "id": "...", "key_prefix": "sb_xxxxxxx", ... }, "secret": "sb_…" }`}</Code>

      <h4>
        <Method m="GET" /> <span className="mono">/api/keys</span>
      </h4>
      <p>List your keys (prefixes and last-used timestamps only; never the secret).</p>

      <h4>
        <Method m="DELETE" /> <span className="mono">/api/keys/:id</span>
      </h4>
      <p>
        Revoke a key. Any client using it stops working immediately. Returns{" "}
        <code>{`{ "ok": true }`}</code>.
      </p>

      {/* ------------------------------------------------------------------ */}
      <h3 id="repos">Repositories</h3>
      <h4>
        <Method m="GET" /> <span className="mono">/api/repos</span>
      </h4>
      <p>
        Lists the GitHub repositories the authenticated user can access, fetched
        live from GitHub. Use a repo's <code>full_name</code> when creating a
        project. Each entry includes <code>id</code>, <code>full_name</code>,{" "}
        <code>private</code>, <code>default_branch</code>, and{" "}
        <code>html_url</code>.
      </p>

      {/* ------------------------------------------------------------------ */}
      <h3 id="projects">Projects</h3>

      <h4>
        <Method m="POST" /> <span className="mono">/api/projects</span>
      </h4>
      <p>Create a project that links a repo to its build settings.</p>
      <table>
        <thead>
          <tr>
            <th>Field</th>
            <th>Type</th>
            <th>Default</th>
            <th>Notes</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>
              <code>name</code>
            </td>
            <td>string</td>
            <td>—</td>
            <td>Required. Display name.</td>
          </tr>
          <tr>
            <td>
              <code>repo_full_name</code>
            </td>
            <td>string</td>
            <td>—</td>
            <td>
              Required. <code>owner/repo</code> on GitHub.
            </td>
          </tr>
          <tr>
            <td>
              <code>repo_id</code>
            </td>
            <td>number</td>
            <td>null</td>
            <td>Optional GitHub numeric repo id.</td>
          </tr>
          <tr>
            <td>
              <code>default_branch</code>
            </td>
            <td>string</td>
            <td>
              <code>main</code>
            </td>
            <td>Branch whose HEAD is used when a build omits a commit.</td>
          </tr>
          <tr>
            <td>
              <code>dockerfile_path</code>
            </td>
            <td>string</td>
            <td>
              <code>Dockerfile</code>
            </td>
            <td>Path to the Dockerfile within the repo.</td>
          </tr>
          <tr>
            <td>
              <code>container_port</code>
            </td>
            <td>number</td>
            <td>
              <code>8080</code>
            </td>
            <td>Port your container listens on; mapped to the public URL.</td>
          </tr>
        </tbody>
      </table>

      <h4>
        <Method m="GET" /> <span className="mono">/api/projects</span>
      </h4>
      <p>List your projects, newest first.</p>

      <h4>
        <Method m="GET" /> <span className="mono">/api/projects/:id</span>
      </h4>
      <p>Fetch a single project by id.</p>

      {/* ------------------------------------------------------------------ */}
      <h3 id="builds">Builds</h3>

      <h4>
        <Method m="POST" /> <span className="mono">/api/projects/:id/builds</span>
      </h4>
      <p>
        Queue a build. The request body is optional.
      </p>
      <table>
        <thead>
          <tr>
            <th>Field</th>
            <th>Type</th>
            <th>Notes</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>
              <code>commit_sha</code>
            </td>
            <td>string</td>
            <td>
              Optional. Build this exact commit. If omitted or empty, the latest
              commit on the project's <code>default_branch</code> is resolved
              from GitHub.
            </td>
          </tr>
        </tbody>
      </table>
      <p>
        Returns the created build with <code>status: "queued"</code>.
      </p>

      <h4>
        <Method m="GET" /> <span className="mono">/api/projects/:id/builds</span>
      </h4>
      <p>List a project's builds, newest first.</p>

      <h4>
        <Method m="GET" /> <span className="mono">/api/builds/:id</span>
      </h4>
      <p>
        Fetch a single build. Poll this to watch progress and stream logs.
      </p>

      {/* ------------------------------------------------------------------ */}
      <h3 id="build-object">The build object</h3>
      <p>Builds move through these statuses:</p>
      <p>
        <span className="badge queued">queued</span>{" "}
        <span className="badge running">running</span>{" "}
        <span className="badge succeeded">succeeded</span>{" "}
        <span className="badge failed">failed</span>
      </p>
      <table>
        <thead>
          <tr>
            <th>Field</th>
            <th>Type</th>
            <th>Notes</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>id</code></td>
            <td>uuid</td>
            <td>Build id.</td>
          </tr>
          <tr>
            <td><code>project_id</code></td>
            <td>uuid</td>
            <td>Parent project.</td>
          </tr>
          <tr>
            <td><code>commit_sha</code></td>
            <td>string</td>
            <td>The commit that was built.</td>
          </tr>
          <tr>
            <td><code>status</code></td>
            <td>string</td>
            <td>
              <code>queued</code> · <code>running</code> ·{" "}
              <code>succeeded</code> · <code>failed</code>.
            </td>
          </tr>
          <tr>
            <td><code>url</code></td>
            <td>string | null</td>
            <td>Public deployment URL once the build succeeds.</td>
          </tr>
          <tr>
            <td><code>sprite_name</code></td>
            <td>string | null</td>
            <td>The sprite the build ran in.</td>
          </tr>
          <tr>
            <td><code>logs</code></td>
            <td>string</td>
            <td>Build log, updated as the build progresses (secrets redacted).</td>
          </tr>
          <tr>
            <td><code>error</code></td>
            <td>string | null</td>
            <td>Failure reason when <code>status</code> is <code>failed</code>.</td>
          </tr>
          <tr>
            <td><code>metadata</code></td>
            <td>object</td>
            <td>Sprite/commit/repo/port details and readiness.</td>
          </tr>
          <tr>
            <td>
              <code>created_at</code>, <code>started_at</code>,{" "}
              <code>finished_at</code>
            </td>
            <td>timestamp</td>
            <td>Lifecycle timestamps (the latter two may be null).</td>
          </tr>
        </tbody>
      </table>

      {/* ------------------------------------------------------------------ */}
      <h3 id="admin">Admin</h3>
      <p>
        Admin-only endpoints give app-wide visibility across every user. They
        require the <code>view_admin_dashboard</code> capability (the{" "}
        <code>admin</code> role); other users receive <code>403</code>. Bootstrap
        the first admin with the <code>ADMIN_GITHUB_LOGINS</code> env var, then
        manage roles from the dashboard.
      </p>

      <h4>
        <Method m="GET" /> <span className="mono">/api/admin/stats</span>
      </h4>
      <p>App-wide counts: users, projects, and builds by status.</p>

      <h4>
        <Method m="GET" /> <span className="mono">/api/admin/builds</span>
      </h4>
      <p>
        Every build across all users, newest first, with owner/project context.
        Optional query params: <code>status</code> (
        <code>queued|running|succeeded|failed</code>) and <code>limit</code>{" "}
        (default 200, max 1000).
      </p>

      <h4>
        <Method m="GET" /> <span className="mono">/api/admin/users</span>
      </h4>
      <p>All users with their role and per-user project/build counts.</p>

      <h4>
        <Method m="PATCH" /> <span className="mono">/api/admin/users/:id/role</span>
      </h4>
      <p>
        Set a user's role. Body: <code>{`{ "role": "user" | "admin" }`}</code>.
        You can't change your own role.
      </p>

      {/* ------------------------------------------------------------------ */}
      <h3 id="errors">Errors</h3>
      <p>
        Errors return the matching HTTP status with a JSON body of the form{" "}
        <code>{`{ "error": "message" }`}</code>.
      </p>
      <table>
        <thead>
          <tr>
            <th>Status</th>
            <th>Meaning</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td><code>400</code></td>
            <td>Bad request — missing/invalid fields, or HEAD couldn't be resolved.</td>
          </tr>
          <tr>
            <td><code>401</code></td>
            <td>Unauthorized — missing or invalid key/session.</td>
          </tr>
          <tr>
            <td><code>403</code></td>
            <td>Forbidden — the resource belongs to another user.</td>
          </tr>
          <tr>
            <td><code>404</code></td>
            <td>Not found.</td>
          </tr>
          <tr>
            <td><code>500</code></td>
            <td>Internal error.</td>
          </tr>
        </tbody>
      </table>
    </div>
  );
}
