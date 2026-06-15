// Shared, presentational build UI: the live log viewer, the diagnostics grid,
// and the small status/duration helpers. Used by both the inline panel on the
// project page and the dedicated build show page (/builds/:id), so the live-log
// rendering lives in exactly one place.
import { ReactNode, useEffect, useRef, useState } from "react";
import { Build } from "../api";

export const isActive = (b: Build) => b.status === "queued" || b.status === "running";

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

/// Diagnostics grid + error + live logs for a single build. Caller owns the
/// surrounding chrome (card, header, close button, page layout).
export function BuildBody({ build, now }: { build: Build; now: number }) {
  const ready = build.metadata?.ready;
  const live = isActive(build);

  return (
    <>
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
