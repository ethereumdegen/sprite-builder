import { useEffect } from "react";
import { Link } from "react-router-dom";
import { AdminBuild } from "../api";
import { useAdmin } from "../stores/admin";
import { useAuth } from "../stores/auth";

const STATUS_FILTERS = ["", "queued", "running", "succeeded", "failed"] as const;

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
  const {
    stats,
    builds,
    users,
    statusFilter,
    loading,
    setStatusFilter,
    loadStats,
    loadBuilds,
    loadUsers,
    setRole,
  } = useAdmin();
  const me = useAuth((s) => s.user);

  // Initial load + light auto-refresh of the live cross-tenant view.
  useEffect(() => {
    loadStats().catch(() => {});
    loadUsers().catch(() => {});
  }, [loadStats, loadUsers]);

  useEffect(() => {
    loadBuilds().catch(() => {});
    const t = setInterval(() => {
      loadStats().catch(() => {});
      loadBuilds().catch(() => {});
    }, 5000);
    return () => clearInterval(t);
  }, [statusFilter, loadBuilds, loadStats]);

  return (
    <div className="admin">
      <h2>Admin</h2>
      <p className="muted">App-wide build activity and diagnostics across all users.</p>

      {/* stats */}
      <div className="stat-grid">
        <Metric k="Users" v={stats?.users} />
        <Metric k="Projects" v={stats?.projects} />
        <Metric k="Builds" v={stats?.builds_total} />
        <Metric k="Queued" v={stats?.builds_queued} />
        <Metric k="Running" v={stats?.builds_running} />
        <Metric k="Succeeded" v={stats?.builds_succeeded} />
        <Metric k="Failed" v={stats?.builds_failed} />
      </div>

      {/* builds */}
      <div className="row" style={{ justifyContent: "space-between", marginTop: 16 }}>
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
              </tr>
            </thead>
            <tbody>
              {builds.map((b) => (
                <tr key={b.id}>
                  <td>
                    <span className={"badge " + b.status}>{b.status}</span>
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
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* users / roles */}
      <h3>Users &amp; roles</h3>
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
    </div>
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
