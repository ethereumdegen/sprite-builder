import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api, Project } from "../api";
import { useBuilds } from "../stores/builds";
import { buildDuration, isActive } from "../components/build";

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
