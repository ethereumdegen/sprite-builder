import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api, Project } from "../api";
import { useBuilds } from "../stores/builds";
import { useCodespaces } from "../stores/codespaces";
import { buildDuration, isActive } from "../components/build";
import { VariablesEditor } from "../components/variables";

// ---------------------------------------------------------------------------
// page
// ---------------------------------------------------------------------------

export default function ProjectPage() {
  const { id } = useParams<{ id: string }>();
  // Select the stable map and derive the list outside the selector — returning a
  // fresh [] from the selector would change identity every render.
  const byProject = useBuilds((s) => s.byProject);
  const loadForProject = useBuilds((s) => s.loadForProject);
  const createBuild = useBuilds((s) => s.create);
  const builds = useMemo(() => (id && byProject[id]) || [], [id, byProject]);

  const navigate = useNavigate();
  const [project, setProject] = useState<Project | null>(null);
  const [triggering, setTriggering] = useState(false);
  const [commit, setCommit] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());

  const loadBuilds = useCallback(() => {
    if (id) loadForProject(id).catch(() => {});
  }, [id, loadForProject]);

  useEffect(() => {
    if (!id) return;
    api.project(id).then(setProject).catch(() => {});
    loadBuilds();
  }, [id, loadBuilds]);

  // Poll the list quickly while a build is in flight; keep a 1s clock for live durations.
  useEffect(() => {
    const active = builds.some(isActive);
    const t = setInterval(
      () => {
        setNow(Date.now());
        if (active) loadBuilds();
      },
      active ? 2000 : 15000
    );
    return () => clearInterval(t);
  }, [builds, loadBuilds]);

  const trigger = async () => {
    if (!id) return;
    setTriggering(true);
    setError(null);
    try {
      const b = await createBuild(id, commit);
      setCommit("");
      navigate(`/builds/${b.id}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setTriggering(false);
    }
  };

  if (!project) return <p className="muted">Loading…</p>;

  return (
    <div>
      <Link to="/" className="muted">
        ← Projects
      </Link>
      <div className="row" style={{ justifyContent: "space-between", marginTop: 8 }}>
        <div>
          <h2 style={{ marginBottom: 4 }}>{project.name}</h2>
          <div className="muted mono">
            {project.repo_full_name} · {project.default_branch} · Dockerfile:{" "}
            {project.dockerfile_path} · :{project.container_port}
          </div>
        </div>
      </div>

      <div className="card">
        <h3 style={{ marginTop: 0 }}>Trigger a build</h3>
        <p className="muted">
          Leave the commit blank to build the HEAD of <b>{project.default_branch}</b>.
        </p>
        <div className="row">
          <input
            placeholder="commit sha (optional)"
            value={commit}
            onChange={(e) => setCommit(e.target.value)}
          />
          <button onClick={trigger} disabled={triggering} style={{ whiteSpace: "nowrap" }}>
            {triggering ? "Queuing…" : "New build"}
          </button>
        </div>
        {error && <p style={{ color: "var(--red)" }}>{error}</p>}
      </div>

      <VariablesEditor projectId={project.id} />

      <CodespacesSection projectId={project.id} branch={project.default_branch} />

      <h3>Builds</h3>
      {builds.length === 0 ? (
        <p className="muted">No builds yet.</p>
      ) : (
        <div className="card">
          {builds.map((b) => (
            <div className="list-item" key={b.id}>
              <div>
                <span className={"badge " + b.status}>{b.status}</span>{" "}
                {isActive(b) && <span className="spin">⟳</span>}{" "}
                <span className="mono">{b.commit_sha.slice(0, 10)}</span>
                <div className="muted">
                  {new Date(b.created_at).toLocaleString()} · {buildDuration(b, now)}
                </div>
              </div>
              <div className="row">
                {b.url && b.status === "succeeded" && (
                  <a href={b.url} target="_blank" rel="noreferrer">
                    Open ↗
                  </a>
                )}
                <Link className="secondary" to={`/builds/${b.id}`}>
                  Details
                </Link>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// codespaces section
// ---------------------------------------------------------------------------

/// Ephemeral coding filesystems for the project: create one (clones the default
/// branch into a sprite) and jump into its editor. Polls while any is still
/// provisioning.
function CodespacesSection({ projectId, branch }: { projectId: string; branch: string }) {
  const byProject = useCodespaces((s) => s.byProject);
  const loadForProject = useCodespaces((s) => s.loadForProject);
  const createCodespace = useCodespaces((s) => s.create);
  const codespaces = useMemo(() => byProject[projectId] || [], [projectId, byProject]);

  const navigate = useNavigate();
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    loadForProject(projectId).catch(() => {});
  }, [projectId, loadForProject]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Poll while any codespace is still provisioning.
  useEffect(() => {
    const pending = codespaces.some((c) => c.status === "queued" || c.status === "provisioning");
    if (!pending) return;
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [codespaces, refresh]);

  const create = async () => {
    setCreating(true);
    setError(null);
    try {
      const cs = await createCodespace(projectId);
      navigate(`/codespaces/${cs.id}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div className="card">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h3 style={{ marginTop: 0, marginBottom: 4 }}>Codespaces</h3>
        <button onClick={create} disabled={creating} style={{ whiteSpace: "nowrap" }}>
          {creating ? "Creating…" : "New codespace"}
        </button>
      </div>
      <p className="muted">
        An ephemeral coding filesystem — clones <b>{branch}</b> into a live sandbox you can edit,
        commit, and push back to GitHub.
      </p>
      {error && <p style={{ color: "var(--red)" }}>{error}</p>}
      {codespaces.length === 0 ? (
        <p className="muted">No codespaces yet.</p>
      ) : (
        codespaces.map((c) => (
          <div className="list-item" key={c.id}>
            <div>
              <span className={"badge " + c.status}>{c.status}</span>{" "}
              {(c.status === "queued" || c.status === "provisioning") && (
                <span className="spin">⟳</span>
              )}{" "}
              <span className="mono">{c.name}</span>
              <div className="muted">
                {c.branch} · {new Date(c.created_at).toLocaleString()}
              </div>
            </div>
            <Link className="secondary" to={`/codespaces/${c.id}`}>
              Open
            </Link>
          </div>
        ))
      )}
    </div>
  );
}
