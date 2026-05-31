import { ReactNode, useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api, Build, Project } from "../api";

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

const isActive = (b: Build) => b.status === "queued" || b.status === "running";

function fmtDuration(ms: number): string {
  if (ms < 0) ms = 0;
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${s % 60}s`;
}

function buildDuration(b: Build, now: number): string {
  if (!b.started_at) return "—";
  const start = new Date(b.started_at).getTime();
  const end = b.finished_at ? new Date(b.finished_at).getTime() : now;
  return fmtDuration(end - start);
}

function classifyLine(line: string): string {
  if (line.includes("__SB_BUILD_OK__")) return "ok";
  const l = line.toLowerCase();
  if (/(^|\W)(error|fatal|failed|cannot|denied|panic|exit code [1-9])/.test(l)) return "err";
  if (/(^|\W)(warn|warning)/.test(l)) return "warn";
  if (line.startsWith("==>") || line.startsWith("---")) return "step";
  return "";
}

// ---------------------------------------------------------------------------
// page
// ---------------------------------------------------------------------------

export default function ProjectPage() {
  const { id } = useParams<{ id: string }>();
  const [project, setProject] = useState<Project | null>(null);
  const [builds, setBuilds] = useState<Build[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [triggering, setTriggering] = useState(false);
  const [commit, setCommit] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());

  const loadBuilds = () => id && api.builds(id).then(setBuilds).catch(() => {});

  useEffect(() => {
    if (!id) return;
    api.project(id).then(setProject).catch(() => {});
    loadBuilds();
  }, [id]);

  // Poll the list quickly while a build is in flight; keep a 1s clock for live durations.
  useEffect(() => {
    const active = builds.some(isActive);
    const t = setInterval(() => {
      setNow(Date.now());
      if (active) loadBuilds();
    }, active ? 2000 : 15000);
    return () => clearInterval(t);
  }, [builds, id]);

  const trigger = async () => {
    if (!id) return;
    setTriggering(true);
    setError(null);
    try {
      const b = await api.createBuild(id, commit.trim() || undefined);
      setCommit("");
      setSelectedId(b.id);
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
  const [build, setBuild] = useState<Build | null>(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    let alive = true;
    const fetchOnce = () => api.build(buildId).then((b) => alive && setBuild(b)).catch(() => {});
    fetchOnce();
    const t = setInterval(() => {
      setNow(Date.now());
      api
        .build(buildId)
        .then((b) => {
          if (!alive) return;
          setBuild(b);
        })
        .catch(() => {});
    }, 1500);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [buildId]);

  if (!build) return null;

  const ready = (build.metadata as any)?.ready;
  const live = isActive(build);

  return (
    <div className="card">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h3 style={{ margin: 0 }}>
          Build <span className="mono">{build.id.slice(0, 8)}</span>{" "}
          <span className={"badge " + build.status}>{build.status}</span>{" "}
          {live && <span className="spin">⟳</span>}
        </h3>
        <button className="secondary" onClick={onClose}>
          Close
        </button>
      </div>

      <div className="stat-grid">
        <Stat k="Commit" v={<span className="mono">{build.commit_sha.slice(0, 12)}</span>} />
        <Stat k="Duration" v={buildDuration(build, now)} />
        <Stat k="Sprite" v={<span className="mono">{build.sprite_name || "—"}</span>} />
        <Stat
          k="Reachable"
          v={
            ready === true ? (
              <span style={{ color: "var(--green)" }}>✓ yes</span>
            ) : ready === false ? (
              <span style={{ color: "var(--red)" }}>✗ no</span>
            ) : (
              "—"
            )
          }
        />
      </div>

      <Stat
        k="URL"
        v={
          build.url ? (
            <a href={build.url} target="_blank" rel="noreferrer">
              {build.url}
            </a>
          ) : (
            "—"
          )
        }
        block
      />

      {build.error && (
        <>
          <label>Error</label>
          <div className="logwrap">
            <div className="logbox">
              <div className="log-line err">{build.error}</div>
            </div>
          </div>
        </>
      )}

      <label>Logs</label>
      <LogView text={build.logs} live={live} />
    </div>
  );
}

function Stat({ k, v, block }: { k: string; v: ReactNode; block?: boolean }) {
  return (
    <div className="stat" style={block ? { marginBottom: 12 } : undefined}>
      <div className="k">{k}</div>
      <div className="v">{v}</div>
    </div>
  );
}

function LogView({ text, live }: { text: string; live: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState(true);

  useEffect(() => {
    if (follow && ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [text, follow]);

  const onScroll = () => {
    const el = ref.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    if (atBottom !== follow) setFollow(atBottom);
  };

  const lines = (text || "").split("\n");

  return (
    <div className="logwrap">
      <div className="logbar">
        <span className="muted">
          {lines.length} lines {live && <span className="pulse">● live</span>}
        </span>
        <div className="row">
          <label>
            <input
              type="checkbox"
              checked={follow}
              onChange={(e) => setFollow(e.target.checked)}
            />
            follow
          </label>
          <button className="secondary" onClick={() => navigator.clipboard?.writeText(text)}>
            Copy
          </button>
        </div>
      </div>
      <div className="logbox" ref={ref} onScroll={onScroll}>
        {text ? (
          lines.map((ln, i) => {
            const display = ln.includes("__SB_BUILD_OK__")
              ? "✓ build completed successfully"
              : ln;
            return (
              <div key={i} className={"log-line " + classifyLine(ln)}>
                {display || " "}
              </div>
            );
          })
        ) : (
          <div className="log-line muted">
            {live ? "waiting for output…" : "(no output)"}
          </div>
        )}
      </div>
    </div>
  );
}
