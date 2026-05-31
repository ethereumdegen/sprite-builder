import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, Project, Repo } from "../api";

export default function ProjectsPage() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);

  const load = () =>
    api
      .projects()
      .then(setProjects)
      .finally(() => setLoading(false));

  useEffect(() => {
    load();
  }, []);

  return (
    <div>
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h2>Projects</h2>
        <button onClick={() => setCreating((v) => !v)}>
          {creating ? "Cancel" : "New project"}
        </button>
      </div>

      {creating && (
        <NewProject
          onCreated={() => {
            setCreating(false);
            setLoading(true);
            load();
          }}
        />
      )}

      {loading ? (
        <p className="muted">Loading…</p>
      ) : projects.length === 0 ? (
        <p className="muted">No projects yet. Create one to pick a repo and start building.</p>
      ) : (
        <div className="card">
          {projects.map((p) => (
            <div className="list-item" key={p.id}>
              <div>
                <Link to={`/projects/${p.id}`}>
                  <strong>{p.name}</strong>
                </Link>
                <div className="muted mono">{p.repo_full_name}</div>
              </div>
              <span className="muted">{p.default_branch}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function NewProject({ onCreated }: { onCreated: () => void }) {
  const [repos, setRepos] = useState<Repo[]>([]);
  const [loadingRepos, setLoadingRepos] = useState(true);
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<Repo | null>(null);
  const [name, setName] = useState("");
  const [dockerfile, setDockerfile] = useState("Dockerfile");
  const [port, setPort] = useState(8080);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .repos()
      .then(setRepos)
      .catch((e) => setError(String(e.message || e)))
      .finally(() => setLoadingRepos(false));
  }, []);

  const pick = (r: Repo) => {
    setSelected(r);
    if (!name) setName(r.name);
  };

  const submit = async () => {
    if (!selected) return;
    setSubmitting(true);
    setError(null);
    try {
      await api.createProject({
        name: name || selected.name,
        repo_full_name: selected.full_name,
        repo_id: selected.id,
        default_branch: selected.default_branch,
        dockerfile_path: dockerfile,
        container_port: port,
      });
      onCreated();
    } catch (e: any) {
      setError(String(e.message || e));
      setSubmitting(false);
    }
  };

  const shown = repos.filter((r) =>
    r.full_name.toLowerCase().includes(filter.toLowerCase())
  );

  return (
    <div className="card">
      <h3 style={{ marginTop: 0 }}>New project</h3>
      {error && <p style={{ color: "var(--red)" }}>{error}</p>}

      <label>Pick a repository</label>
      <input
        placeholder="Filter repos…"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
      <div className="repo-picker" style={{ marginTop: 8 }}>
        {loadingRepos ? (
          <div className="repo-option muted">Loading repos…</div>
        ) : (
          shown.slice(0, 200).map((r) => (
            <div
              key={r.id}
              className={"repo-option" + (selected?.id === r.id ? " selected" : "")}
              onClick={() => pick(r)}
            >
              <strong>{r.full_name}</strong>
              {r.private && <span className="muted"> · private</span>}
              {r.description && <div className="muted">{r.description}</div>}
            </div>
          ))
        )}
      </div>

      {selected && (
        <>
          <div className="grid2" style={{ marginTop: 12 }}>
            <div>
              <label>Project name</label>
              <input value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            <div>
              <label>Default branch</label>
              <input value={selected.default_branch} disabled />
            </div>
            <div>
              <label>Dockerfile path</label>
              <input value={dockerfile} onChange={(e) => setDockerfile(e.target.value)} />
            </div>
            <div>
              <label>Container port (mapped to sprite :8080)</label>
              <input
                type="number"
                value={port}
                onChange={(e) => setPort(Number(e.target.value))}
              />
            </div>
          </div>
          <div style={{ marginTop: 12 }}>
            <button onClick={submit} disabled={submitting}>
              {submitting ? "Creating…" : "Create project"}
            </button>
          </div>
        </>
      )}
    </div>
  );
}
