// Codespace detail page (/codespaces/:id): provisioning status + a minimal
// file browser / editor and a git (commit / push / pull) panel, all driven by
// the synchronous codespace file/exec/git API. v1 has no terminal box (exec is
// API-only) and uses plain textareas rather than a full editor.
import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api, FileEntry } from "../api";
import { useCodespaces } from "../stores/codespaces";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
const parentPath = (p: string) => {
  const i = p.lastIndexOf("/");
  return i < 0 ? "" : p.slice(0, i);
};
const joinPath = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);

interface OpenFile {
  path: string;
  binary: boolean;
  truncated: boolean;
  size: number;
}

export default function CodespacePage() {
  const { id } = useParams<{ id: string }>();
  const cs = useCodespaces((s) => (id ? s.byId[id] : undefined));
  const load = useCodespaces((s) => s.load);
  const remove = useCodespaces((s) => s.remove);
  const navigate = useNavigate();

  const [missing, setMissing] = useState(false);
  const [dir, setDir] = useState("");
  const [entries, setEntries] = useState<FileEntry[] | null>(null);
  const [file, setFile] = useState<OpenFile | null>(null);
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [commitMsg, setCommitMsg] = useState("");
  const [gitOut, setGitOut] = useState("");
  const [gitBusy, setGitBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const ready = cs?.status === "ready";

  // Poll the record until it reaches a terminal state (ready/failed).
  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    let timer: number | undefined;
    const tick = async () => {
      try {
        const c = await load(id);
        if (cancelled) return;
        if (c.status !== "ready" && c.status !== "failed") {
          timer = window.setTimeout(tick, 1500);
        }
      } catch {
        if (!cancelled) setMissing(true);
      }
    };
    tick();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [id, load]);

  const loadDir = useCallback(
    async (d: string) => {
      if (!id) return;
      setErr(null);
      try {
        const r = await api.csRead(id, d);
        if (r.kind === "dir") {
          setEntries(r.entries || []);
          setDir(d);
        }
      } catch (e) {
        setErr(msg(e));
      }
    },
    [id]
  );

  // Load the workspace root once the codespace is ready.
  useEffect(() => {
    if (ready && entries === null) loadDir("");
  }, [ready, entries, loadDir]);

  const openFile = async (path: string) => {
    if (!id) return;
    setErr(null);
    try {
      const r = await api.csRead(id, path);
      if (r.kind === "file") {
        setFile({ path, binary: r.binary, truncated: r.truncated, size: r.size });
        setContent(r.binary ? "" : r.content || "");
        setDirty(false);
      } else {
        loadDir(path);
      }
    } catch (e) {
      setErr(msg(e));
    }
  };

  const save = async () => {
    if (!id || !file) return;
    setSaving(true);
    setErr(null);
    try {
      await api.csWrite(id, file.path, content);
      setDirty(false);
    } catch (e) {
      setErr(msg(e));
    } finally {
      setSaving(false);
    }
  };

  const runGit = async (
    op: "status" | "diff" | "commit" | "push" | "pull",
    message?: string
  ) => {
    if (!id) return;
    setGitBusy(true);
    setErr(null);
    try {
      const r = await api.csGit(id, op, message);
      setGitOut(`$ git ${op}${r.ok ? "" : ` — exit ${r.exit_code}`}\n${r.output || "(no output)"}`);
      if (op === "commit") setCommitMsg("");
    } catch (e) {
      setErr(msg(e));
    } finally {
      setGitBusy(false);
    }
  };

  const destroy = async () => {
    if (!id || !cs) return;
    if (!window.confirm("Delete this codespace and tear down its sprite?")) return;
    try {
      await remove(id, cs.project_id);
      navigate(`/projects/${cs.project_id}`);
    } catch (e) {
      setErr(msg(e));
    }
  };

  if (missing) {
    return (
      <div>
        <p className="muted">Codespace not found.</p>
        <Link to="/">← Projects</Link>
      </div>
    );
  }
  if (!cs) return <p className="muted">Loading…</p>;

  return (
    <div>
      <div className="row" style={{ marginBottom: 8 }}>
        <Link to={`/projects/${cs.project_id}`}>← Back to project</Link>
      </div>

      <div className="card">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h2 style={{ margin: 0 }}>
            {cs.name} <span className={"badge " + cs.status}>{cs.status}</span>{" "}
            {(cs.status === "queued" || cs.status === "provisioning") && (
              <span className="spin">⟳</span>
            )}
          </h2>
          <button className="secondary" onClick={destroy}>
            Delete
          </button>
        </div>
        <div className="muted mono">
          branch: {cs.branch}
          {cs.sprite_name && <> · sprite: {cs.sprite_name}</>}
        </div>
      </div>

      {err && <p style={{ color: "var(--red)" }}>{err}</p>}

      {(cs.status === "queued" || cs.status === "provisioning") && (
        <div className="card">
          <p className="muted">Provisioning the codespace (creating sprite + cloning repo)…</p>
          {cs.logs && <pre className="logs">{cs.logs}</pre>}
        </div>
      )}

      {cs.status === "failed" && (
        <div className="card">
          <p style={{ color: "var(--red)" }}>Provisioning failed: {cs.error}</p>
          {cs.logs && <pre className="logs">{cs.logs}</pre>}
        </div>
      )}

      {ready && (
        <>
          <div style={{ display: "grid", gridTemplateColumns: "minmax(220px, 1fr) 2fr", gap: 12 }}>
            {/* file browser */}
            <div className="card">
              <div className="row" style={{ justifyContent: "space-between", marginBottom: 8 }}>
                <strong className="mono">/{dir}</strong>
                <button className="secondary" onClick={() => loadDir(dir)}>
                  Refresh
                </button>
              </div>
              {dir && (
                <div
                  className="list-item"
                  style={{ cursor: "pointer" }}
                  onClick={() => loadDir(parentPath(dir))}
                >
                  <span className="mono">../</span>
                </div>
              )}
              {(entries || []).map((e) => (
                <div
                  className="list-item"
                  key={e.name}
                  style={{ cursor: "pointer" }}
                  onClick={() =>
                    e.is_dir ? loadDir(joinPath(dir, e.name)) : openFile(joinPath(dir, e.name))
                  }
                >
                  <span className="mono">
                    {e.is_dir ? "📁 " : "📄 "}
                    {e.name}
                    {e.is_dir ? "/" : ""}
                  </span>
                </div>
              ))}
              {entries && entries.length === 0 && <p className="muted">empty</p>}
            </div>

            {/* editor */}
            <div className="card">
              {!file ? (
                <p className="muted">Select a file to view or edit.</p>
              ) : file.binary ? (
                <p className="muted">
                  <span className="mono">{file.path}</span> — binary file ({file.size} bytes), not
                  editable here.
                </p>
              ) : (
                <>
                  <div className="row" style={{ justifyContent: "space-between", marginBottom: 8 }}>
                    <strong className="mono">{file.path}</strong>
                    <div className="row">
                      {file.truncated && <span className="muted">truncated (1 MiB cap)</span>}
                      <button onClick={save} disabled={!dirty || saving}>
                        {saving ? "Saving…" : "Save"}
                      </button>
                    </div>
                  </div>
                  <textarea
                    className="mono"
                    style={{ width: "100%", minHeight: 360 }}
                    value={content}
                    spellCheck={false}
                    onChange={(ev) => {
                      setContent(ev.target.value);
                      setDirty(true);
                    }}
                  />
                </>
              )}
            </div>
          </div>

          {/* git panel */}
          <div className="card">
            <h3 style={{ marginTop: 0 }}>Git</h3>
            <p className="muted">
              Commits and pushes go to <b>{cs.branch}</b> on the project's GitHub repo.
            </p>
            <div className="row">
              <input
                placeholder="commit message"
                value={commitMsg}
                onChange={(e) => setCommitMsg(e.target.value)}
                style={{ flex: 1 }}
              />
              <button
                onClick={() => runGit("commit", commitMsg)}
                disabled={gitBusy || !commitMsg.trim()}
                style={{ whiteSpace: "nowrap" }}
              >
                Commit
              </button>
            </div>
            <div className="row" style={{ marginTop: 8 }}>
              <button className="secondary" onClick={() => runGit("status")} disabled={gitBusy}>
                Status
              </button>
              <button className="secondary" onClick={() => runGit("diff")} disabled={gitBusy}>
                Diff
              </button>
              <button onClick={() => runGit("push")} disabled={gitBusy}>
                Push
              </button>
              <button className="secondary" onClick={() => runGit("pull")} disabled={gitBusy}>
                Pull
              </button>
              {gitBusy && <span className="spin">⟳</span>}
            </div>
            {gitOut && <pre className="logs">{gitOut}</pre>}
          </div>
        </>
      )}
    </div>
  );
}
