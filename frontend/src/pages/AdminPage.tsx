import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { AdminBuild, AdminSprite } from "../api";
import { useAdmin } from "../stores/admin";
import { useAuth } from "../stores/auth";

const STATUS_FILTERS = ["", "queued", "running", "succeeded", "failed"] as const;

const TABS = [
  { key: "overview", label: "Overview" },
  { key: "builds", label: "Builds" },
  { key: "sprites", label: "Sprites" },
  { key: "users", label: "Users" },
] as const;

type TabKey = (typeof TABS)[number]["key"];

function fmt(ts: string | null): string {
  return ts ? new Date(ts).toLocaleString() : "—";
}

function duration(b: AdminBuild): string {
  if (!b.started_at) return "—";
  const end = b.finished_at ? new Date(b.finished_at).getTime() : Date.now();
  const s = Math.max(0, Math.floor((end - new Date(b.started_at).getTime()) / 1000));
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}

export default function AdminPage() {
  const [tab, setTab] = useState<TabKey>("overview");

  return (
    <div className="admin">
      <h2>Admin</h2>
      <p className="muted">App-wide build activity and diagnostics across all users.</p>

      <nav className="pills" role="tablist" aria-label="Admin sections">
        {TABS.map((t) => (
          <button
            key={t.key}
            role="tab"
            aria-selected={tab === t.key}
            className={"pill" + (tab === t.key ? " active" : "")}
            onClick={() => setTab(t.key)}
          >
            {t.label}
          </button>
        ))}
      </nav>

      {tab === "overview" && <OverviewTab />}
      {tab === "builds" && <BuildsTab />}
      {tab === "sprites" && <SpritesTab />}
      {tab === "users" && <UsersTab />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

function OverviewTab() {
  const { stats, loadStats } = useAdmin();

  useEffect(() => {
    loadStats().catch(() => {});
    const t = setInterval(() => loadStats().catch(() => {}), 5000);
    return () => clearInterval(t);
  }, [loadStats]);

  return (
    <div className="stat-grid">
      <Metric k="Users" v={stats?.users} />
      <Metric k="Projects" v={stats?.projects} />
      <Metric k="Builds" v={stats?.builds_total} />
      <Metric k="Queued" v={stats?.builds_queued} />
      <Metric k="Running" v={stats?.builds_running} />
      <Metric k="Succeeded" v={stats?.builds_succeeded} />
      <Metric k="Failed" v={stats?.builds_failed} />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Builds
// ---------------------------------------------------------------------------

function BuildsTab() {
  const { builds, statusFilter, loading, setStatusFilter, loadBuilds } = useAdmin();

  useEffect(() => {
    loadBuilds().catch(() => {});
    const t = setInterval(() => loadBuilds().catch(() => {}), 5000);
    return () => clearInterval(t);
  }, [statusFilter, loadBuilds]);

  return (
    <>
      <div className="row" style={{ justifyContent: "space-between", marginTop: 8 }}>
        <h3 style={{ margin: 0 }}>Build jobs</h3>
        <div className="row">
          {STATUS_FILTERS.map((s) => (
            <button
              key={s || "all"}
              className={"secondary" + (statusFilter === s ? " selected" : "")}
              onClick={() => setStatusFilter(s)}
            >
              {s || "all"}
            </button>
          ))}
        </div>
      </div>

      {builds.length === 0 ? (
        <p className="muted">{loading ? "Loading…" : "No builds match."}</p>
      ) : (
        <div className="card" style={{ overflowX: "auto" }}>
          <table>
            <thead>
              <tr>
                <th>Status</th>
                <th>Owner</th>
                <th>Project</th>
                <th>Commit</th>
                <th>Created</th>
                <th>Duration</th>
                <th>Result</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {builds.map((b) => (
                <BuildRow key={b.id} b={b} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

function BuildRow({ b }: { b: AdminBuild }) {
  const rebuild = useAdmin((s) => s.rebuild);
  return (
    <tr>
      <td>
        <Link to={`/builds/${b.id}`} className={"badge " + b.status}>
          {b.status}
        </Link>
      </td>
      <td className="mono">{b.owner_login}</td>
      <td>
        <Link to={`/projects/${b.project_id}`}>{b.project_name}</Link>
        <div className="muted mono">{b.repo_full_name}</div>
      </td>
      <td className="mono">{b.commit_sha.slice(0, 10)}</td>
      <td className="muted">{fmt(b.created_at)}</td>
      <td className="muted">{duration(b)}</td>
      <td>
        {b.url && b.status === "succeeded" ? (
          <a href={b.url} target="_blank" rel="noreferrer">
            Open ↗
          </a>
        ) : b.error ? (
          <span style={{ color: "var(--red)" }} title={b.error}>
            {b.error.length > 60 ? b.error.slice(0, 60) + "…" : b.error}
          </span>
        ) : (
          "—"
        )}
      </td>
      <td>
        <button
          className="secondary"
          title={`Re-run commit ${b.commit_sha.slice(0, 10)}`}
          onClick={() => rebuild(b.id).catch(() => {})}
        >
          Rebuild
        </button>
      </td>
    </tr>
  );
}

// ---------------------------------------------------------------------------
// Sprites (live sprites.dev inventory)
// ---------------------------------------------------------------------------

function SpritesTab() {
  const { sprites, spritesLoading, loadSprites } = useAdmin();

  useEffect(() => {
    loadSprites().catch(() => {});
  }, [loadSprites]);

  return (
    <>
      <div className="row" style={{ justifyContent: "space-between", marginTop: 8 }}>
        <h3 style={{ margin: 0 }}>Sprites on sprites.dev</h3>
        <button className="secondary" onClick={() => loadSprites().catch(() => {})}>
          Refresh
        </button>
      </div>
      <p className="muted" style={{ marginTop: 4 }}>
        Live VMs provisioned on sprites.dev. Each build runs on a sprite;
        unreferenced (orphaned) sprites can be reclaimed here.
      </p>

      {sprites.length === 0 ? (
        <p className="muted">{spritesLoading ? "Loading…" : "No active sprites."}</p>
      ) : (
        <div className="card" style={{ overflowX: "auto" }}>
          <table>
            <thead>
              <tr>
                <th>Sprite</th>
                <th>State</th>
                <th>Owner</th>
                <th>Project</th>
                <th>Build</th>
                <th>Created</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {sprites.map((sp) => (
                <SpriteRow key={sp.name} sp={sp} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

function SpriteRow({ sp }: { sp: AdminSprite }) {
  const { deleteSprite, setSpritePublic } = useAdmin();

  const onDelete = () => {
    if (window.confirm(`Delete sprite "${sp.name}" on sprites.dev? This cannot be undone.`)) {
      deleteSprite(sp.name).catch(() => {});
    }
  };

  return (
    <tr>
      <td>
        <a href={sp.public_url} target="_blank" rel="noreferrer" className="mono">
          {sp.name}
        </a>
        {sp.orphaned && (
          <div className="badge failed" title="No build references this sprite">
            orphaned
          </div>
        )}
      </td>
      <td>{sp.status ? <span className="badge">{sp.status}</span> : <span className="muted">—</span>}</td>
      <td className="mono">{sp.owner_login ?? <span className="muted">—</span>}</td>
      <td>
        {sp.project_id && sp.project_name ? (
          <Link to={`/projects/${sp.project_id}`}>{sp.project_name}</Link>
        ) : (
          <span className="muted">—</span>
        )}
      </td>
      <td>
        {sp.build_id ? (
          <Link to={`/builds/${sp.build_id}`} className={"badge " + (sp.build_status ?? "")}>
            {sp.build_status ?? "view"}
          </Link>
        ) : (
          <span className="muted">—</span>
        )}
      </td>
      <td className="muted">{fmt(sp.created_at)}</td>
      <td>
        <div className="row" style={{ gap: 6 }}>
          <button
            className="secondary"
            title="Make this sprite's URL publicly reachable"
            onClick={() => setSpritePublic(sp.name).catch(() => {})}
          >
            Make public
          </button>
          <button className="secondary" title="Delete this sprite" onClick={onDelete}>
            Delete
          </button>
        </div>
      </td>
    </tr>
  );
}

// ---------------------------------------------------------------------------
// Users & roles
// ---------------------------------------------------------------------------

function UsersTab() {
  const { users, loadUsers, setRole } = useAdmin();
  const me = useAuth((s) => s.user);

  useEffect(() => {
    loadUsers().catch(() => {});
  }, [loadUsers]);

  return (
    <>
      <h3 style={{ marginTop: 8 }}>Users &amp; roles</h3>
      <div className="card" style={{ overflowX: "auto" }}>
        <table>
          <thead>
            <tr>
              <th>User</th>
              <th>Role</th>
              <th>Projects</th>
              <th>Builds</th>
              <th>Joined</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {users.map((u) => {
              const isSelf = me?.id === u.id;
              const next = u.role === "admin" ? "user" : "admin";
              return (
                <tr key={u.id}>
                  <td>
                    <strong>{u.github_login}</strong>
                    {u.name && <div className="muted">{u.name}</div>}
                  </td>
                  <td>
                    <span className={"badge " + (u.role === "admin" ? "succeeded" : "")}>
                      {u.role}
                    </span>
                  </td>
                  <td>{u.projects}</td>
                  <td>{u.builds}</td>
                  <td className="muted">{fmt(u.created_at)}</td>
                  <td>
                    <button
                      className="secondary"
                      disabled={isSelf}
                      title={isSelf ? "You can't change your own role" : ""}
                      onClick={() => setRole(u.id, next).catch(() => {})}
                    >
                      {u.role === "admin" ? "Demote to user" : "Promote to admin"}
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </>
  );
}

function Metric({ k, v }: { k: string; v: number | undefined }) {
  return (
    <div className="stat">
      <div className="k">{k}</div>
      <div className="v">{v ?? "—"}</div>
    </div>
  );
}
