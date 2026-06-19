// Shared, presentational build UI: the live log viewer, the diagnostics grid,
// and the small status/duration helpers. Used by both the inline panel on the
// project page and the dedicated build show page (/builds/:id), so the live-log
// rendering lives in exactly one place.
import { ReactNode, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Build, DeploymentStatus } from "../api";
import { useBuilds } from "../stores/builds";

export const isActive = (b: Build) => b.status === "queued" || b.status === "running";

/// A build whose admin-deleted sprite we already flagged, without needing a live
/// probe. The live `/deployment` check is still authoritative when available.
export const deploymentRemoved = (b: Build) => b.metadata?.deployment_removed === true;

function fmtDuration(ms: number): string {
  if (ms < 0) ms = 0;
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${s % 60}s`;
}

export function buildDuration(b: Build, now: number): string {
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

export function Stat({ k, v, block }: { k: string; v: ReactNode; block?: boolean }) {
  return (
    <div className="stat" style={block ? { marginBottom: 12 } : undefined}>
      <div className="k">{k}</div>
      <div className="v">{v}</div>
    </div>
  );
}

/// Poll the live deployment status of a succeeded build. The sprite behind a
/// `succeeded` build can be hibernated away or deleted at any time, so a one-shot
/// read goes stale — we re-probe on an interval while the page is open.
function useDeployment(build: Build): DeploymentStatus | undefined {
  const dep = useBuilds((s) => s.deploymentById[build.id]);
  const loadDeployment = useBuilds((s) => s.loadDeployment);
  useEffect(() => {
    if (build.status !== "succeeded") return;
    const tick = () => loadDeployment(build.id).catch(() => {});
    tick();
    const t = setInterval(tick, 10000);
    return () => clearInterval(t);
  }, [build.id, build.status, loadDeployment]);
  return build.status === "succeeded" ? dep : undefined;
}

/// Diagnostics grid + error + live logs for a single build. Caller owns the
/// surrounding chrome (card, header, close button, page layout).
export function BuildBody({ build, now }: { build: Build; now: number }) {
  const dep = useDeployment(build);

  return (
    <>
      <div className="stat-grid">
        <Stat k="Commit" v={<span className="mono">{build.commit_sha.slice(0, 12)}</span>} />
        <Stat k="Duration" v={buildDuration(build, now)} />
        <Stat k="Sprite" v={<span className="mono">{build.sprite_name || "—"}</span>} />
        <Stat k="Deployment" v={<DeploymentBadge build={build} dep={dep} />} />
      </div>

      <DeploymentUrl build={build} dep={dep} />

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

      <LogTabs build={build} />

      <BuildActions build={build} dep={dep} />
    </>
  );
}

/// Live deployment status — the *current* truth, not the build outcome. `live`
/// means the sprite is up (the URL works); `removed` means it's gone (the URL is
/// dead); `unknown` means we couldn't reach sprites.dev, so we don't claim either.
function DeploymentBadge({ build, dep }: { build: Build; dep?: DeploymentStatus }) {
  if (build.status !== "succeeded") return <span className="muted">—</span>;
  const removed = dep?.state === "removed" || deploymentRemoved(build);
  if (removed) return <span style={{ color: "var(--red)" }}>✗ removed</span>;
  if (!dep) return <span className="muted">checking…</span>;
  if (dep.state === "live") return <span style={{ color: "var(--green)" }}>● running</span>;
  if (dep.state === "none") return <span className="muted">none</span>;
  return <span className="muted">unknown</span>;
}

/// The deployment URL row. When the sprite is gone we deliberately do NOT render a
/// live link to a dead URL (which just serves the sprites.dev "private"
/// placeholder); we mark it removed and point at Redeploy instead.
function DeploymentUrl({ build, dep }: { build: Build; dep?: DeploymentStatus }) {
  const removed = dep?.state === "removed" || deploymentRemoved(build);

  if (!build.url) {
    return <Stat k="URL" v="—" block />;
  }

  if (removed) {
    return (
      <Stat
        k="URL"
        v={
          <div>
            <span className="mono" style={{ textDecoration: "line-through", opacity: 0.6 }}>
              {build.url}
            </span>
            <div className="muted" style={{ marginTop: 4, fontSize: 12 }}>
              Deployment removed — the sprite no longer exists. Redeploy to bring it back.
            </div>
          </div>
        }
        block
      />
    );
  }

  return (
    <Stat
      k="URL"
      v={
        <>
          <a href={build.url} target="_blank" rel="noreferrer">
            {build.url}
          </a>
          <UrlVisibilityControl build={build} />
        </>
      }
      block
    />
  );
}

/// Redeploy (rebuild the same commit) and Delete (tear down the sprite + remove
/// the build row). Shown once the build reaches a terminal state. Redeploy is the
/// answer to a removed deployment; Delete is the missing way to clear out a dead
/// or throwaway build instead of leaving it dangling forever.
function BuildActions({ build, dep }: { build: Build; dep?: DeploymentStatus }) {
  const navigate = useNavigate();
  const create = useBuilds((s) => s.create);
  const remove = useBuilds((s) => s.remove);
  const [busy, setBusy] = useState<"redeploy" | "delete" | null>(null);
  const [err, setErr] = useState<string | null>(null);

  if (isActive(build)) return null;

  const removed = dep?.state === "removed" || deploymentRemoved(build);

  const redeploy = async () => {
    setBusy("redeploy");
    setErr(null);
    try {
      const b = await create(build.project_id, build.commit_sha);
      navigate(`/builds/${b.id}`);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setBusy(null);
    }
  };

  const del = async () => {
    if (!window.confirm("Delete this build and tear down its deployment? This can't be undone."))
      return;
    setBusy("delete");
    setErr(null);
    try {
      await remove(build.id, build.project_id);
      navigate(`/projects/${build.project_id}`);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setBusy(null);
    }
  };

  return (
    <div className="row" style={{ gap: 10, marginTop: 14, alignItems: "center" }}>
      <button onClick={redeploy} disabled={busy !== null} style={{ whiteSpace: "nowrap" }}>
        {busy === "redeploy" ? "Queuing…" : removed ? "Redeploy" : "Redeploy this commit"}
      </button>
      <button className="secondary" onClick={del} disabled={busy !== null}>
        {busy === "delete" ? "Deleting…" : "Delete build"}
      </button>
      {err && <span style={{ color: "var(--red)", fontSize: 12 }}>{err}</span>}
    </div>
  );
}

/// Shows whether the deployment's URL is public (anyone) or org-only, with a
/// toggle. Only meaningful for a succeeded build with a live sprite.
function UrlVisibilityControl({ build }: { build: Build }) {
  const vis = useBuilds((s) => s.visibilityById[build.id]);
  const loadVisibility = useBuilds((s) => s.loadVisibility);
  const setVisibility = useBuilds((s) => s.setVisibility);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (build.status === "succeeded") loadVisibility(build.id).catch(() => {});
  }, [build.id, build.status, loadVisibility]);

  if (build.status !== "succeeded" || !vis) return null;
  if (!vis.available) {
    return <div className="muted" style={{ marginTop: 6, fontSize: 12 }}>{vis.message || "visibility unavailable"}</div>;
  }

  const toggle = async () => {
    setBusy(true);
    setErr(null);
    try {
      await setVisibility(build.id, !vis.public);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="row" style={{ alignItems: "center", gap: 10, marginTop: 6 }}>
      <span className={"badge " + (vis.public ? "succeeded" : "queued")}>
        {vis.public ? "🌐 Public" : "🔒 Org only"}
      </span>
      <span className="muted" style={{ fontSize: 12 }}>
        {vis.public ? "anyone with the link can open it" : "only your org can open it"}
      </span>
      <button className="secondary" onClick={toggle} disabled={busy}>
        {busy ? "Updating…" : vis.public ? "Make private" : "Make public"}
      </button>
      {err && <span style={{ color: "var(--red)", fontSize: 12 }}>{err}</span>}
    </div>
  );
}

/// Deploy (build) logs vs Runtime (`docker logs`) logs, as Railway-style tabs.
/// Runtime logs are fetched on demand and polled only while the tab is open.
export function LogTabs({ build }: { build: Build }) {
  const [tab, setTab] = useState<"deploy" | "runtime">("deploy");
  const loadRuntime = useBuilds((s) => s.loadRuntime);
  const runtime = useBuilds((s) => s.runtimeById[build.id]);
  const live = isActive(build);

  useEffect(() => {
    if (tab !== "runtime") return;
    const tick = () => loadRuntime(build.id).catch(() => {});
    tick();
    const t = setInterval(tick, live ? 2000 : 5000);
    return () => clearInterval(t);
  }, [tab, build.id, loadRuntime, live]);

  return (
    <>
      <div className="pills">
        <button
          className={"pill" + (tab === "deploy" ? " active" : "")}
          onClick={() => setTab("deploy")}
        >
          Deploy
        </button>
        <button
          className={"pill" + (tab === "runtime" ? " active" : "")}
          onClick={() => setTab("runtime")}
        >
          Runtime
        </button>
      </div>
      {tab === "deploy" ? (
        <LogView text={build.logs} live={live} />
      ) : runtime && !runtime.available ? (
        <p className="muted">{runtime.message || "Runtime logs are not available."}</p>
      ) : (
        <LogView text={runtime?.logs || ""} live={live} />
      )}
    </>
  );
}

export function LogView({ text, live }: { text: string; live: boolean }) {
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
                {display || " "}
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
