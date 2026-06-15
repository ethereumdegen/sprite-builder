import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api, Project } from "../api";
import { useBuilds } from "../stores/builds";
import { BuildBody, buildDuration, isActive } from "../components/build";

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

  const [project, setProject] = useState<Project | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
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
      setSelectedId(b.id);
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
                <button
                  className="secondary"
                  onClick={() => setSelectedId(selectedId === b.id ? null : b.id)}
                >
                  {selectedId === b.id ? "Hide" : "Details"}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {selectedId && <BuildDetail buildId={selectedId} onClose={() => setSelectedId(null)} />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// build detail + live diagnostics
// ---------------------------------------------------------------------------

function BuildDetail({ buildId, onClose }: { buildId: string; onClose: () => void }) {
  const build = useBuilds((s) => s.byId[buildId]);
  const loadBuild = useBuilds((s) => s.loadBuild);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    loadBuild(buildId).catch(() => {});
    const t = setInterval(() => {
      setNow(Date.now());
      loadBuild(buildId).catch(() => {});
    }, 1500);
    return () => clearInterval(t);
  }, [buildId, loadBuild]);

  if (!build) return null;

  const live = isActive(build);

  return (
    <div className="card">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h3 style={{ margin: 0 }}>
          Build <span className="mono">{build.id.slice(0, 8)}</span>{" "}
          <span className={"badge " + build.status}>{build.status}</span>{" "}
          {live && <span className="spin">⟳</span>}
        </h3>
        <div className="row">
          <Link to={`/builds/${build.id}`}>Open ↗</Link>
          <button className="secondary" onClick={onClose}>
            Close
          </button>
        </div>
      </div>

      <BuildBody build={build} now={now} />
    </div>
  );
}
