import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api, Build, Project } from "../api";

export default function ProjectPage() {
  const { id } = useParams<{ id: string }>();
  const [project, setProject] = useState<Project | null>(null);
  const [builds, setBuilds] = useState<Build[]>([]);
  const [selected, setSelected] = useState<Build | null>(null);
  const [triggering, setTriggering] = useState(false);
  const [commit, setCommit] = useState("");
  const [error, setError] = useState<string | null>(null);

  const loadBuilds = () => id && api.builds(id).then(setBuilds);

  useEffect(() => {
    if (!id) return;
    api.project(id).then(setProject);
    loadBuilds();
  }, [id]);

  // Poll while any build is in flight.
  useEffect(() => {
    const active = builds.some((b) => b.status === "queued" || b.status === "running");
    if (!active) return;
    const t = setInterval(loadBuilds, 3000);
    return () => clearInterval(t);
  }, [builds, id]);

  const trigger = async () => {
    if (!id) return;
    setTriggering(true);
    setError(null);
    try {
      await api.createBuild(id, commit.trim() || undefined);
      setCommit("");
      loadBuilds();
    } catch (e: any) {
      setError(String(e.message || e));
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
            {project.repo_full_name} · {project.default_branch} · :{project.container_port}
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
                <span className="mono">{b.commit_sha.slice(0, 10)}</span>
                <div className="muted">{new Date(b.created_at).toLocaleString()}</div>
              </div>
              <div className="row">
                {b.url && b.status === "succeeded" && (
                  <a href={b.url} target="_blank" rel="noreferrer">
                    Open ↗
                  </a>
                )}
                <button className="secondary" onClick={() => setSelected(b)}>
                  Details
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {selected && <BuildDetail buildId={selected.id} onClose={() => setSelected(null)} />}
    </div>
  );
}

function BuildDetail({ buildId, onClose }: { buildId: string; onClose: () => void }) {
  const [build, setBuild] = useState<Build | null>(null);

  const load = () => api.build(buildId).then(setBuild);

  useEffect(() => {
    load();
    const t = setInterval(() => {
      api.build(buildId).then((b) => {
        setBuild(b);
        if (b.status === "succeeded" || b.status === "failed") clearInterval(t);
      });
    }, 3000);
    return () => clearInterval(t);
  }, [buildId]);

  if (!build) return null;

  return (
    <div className="card">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h3 style={{ margin: 0 }}>
          Build <span className="mono">{build.id.slice(0, 8)}</span>{" "}
          <span className={"badge " + build.status}>{build.status}</span>
        </h3>
        <button className="secondary" onClick={onClose}>
          Close
        </button>
      </div>

      <div className="grid2" style={{ marginTop: 12 }}>
        <div>
          <label>Commit</label>
          <div className="mono">{build.commit_sha}</div>
        </div>
        <div>
          <label>Sprite</label>
          <div className="mono">{build.sprite_name || "—"}</div>
        </div>
        <div>
          <label>URL</label>
          <div>
            {build.url ? (
              <a href={build.url} target="_blank" rel="noreferrer">
                {build.url}
              </a>
            ) : (
              "—"
            )}
          </div>
        </div>
        <div>
          <label>Finished</label>
          <div>{build.finished_at ? new Date(build.finished_at).toLocaleString() : "—"}</div>
        </div>
      </div>

      {build.error && (
        <>
          <label>Error</label>
          <pre className="logs" style={{ color: "var(--red)" }}>
            {build.error}
          </pre>
        </>
      )}

      <label>Logs</label>
      <pre className="logs">{build.logs || "(no output yet)"}</pre>
    </div>
  );
}
