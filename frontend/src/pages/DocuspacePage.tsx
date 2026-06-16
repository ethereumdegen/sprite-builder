// Docuspace detail page (/docuspaces/:id): a recursive file-tree on the left and
// a markdown-rendering viewer/editor on the right, driven by the S3-backed
// docuspace file API. Markdown files render with react-markdown (toggle to edit);
// other text files open in a plain editor; binary files offer a download.
import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, DocEntry } from "../api";
import { useDocuspaces } from "../stores/docuspaces";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
const joinPath = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
const isMarkdown = (p: string) => /\.(md|markdown)$/i.test(p);

interface OpenFile {
  path: string;
  binary: boolean;
  size: number;
  content: string;
}

// --- recursive, lazily-loaded folder tree ---------------------------------

function TreeNode({
  dsId,
  dir,
  name,
  depth,
  activePath,
  onOpenFile,
  onSelectDir,
}: {
  dsId: string;
  dir: string; // full path of this directory ("" = root)
  name: string; // label (root shows "/")
  depth: number;
  activePath: string;
  onOpenFile: (path: string) => void;
  onSelectDir: (path: string) => void;
}) {
  const [expanded, setExpanded] = useState(depth === 0);
  const [loaded, setLoaded] = useState(false);
  const [entries, setEntries] = useState<DocEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const fetchEntries = useCallback(async () => {
    setLoading(true);
    setErr(null);
    try {
      const r = await api.dsRead(dsId, dir);
      setEntries(r.kind === "dir" ? r.entries || [] : []);
      setLoaded(true);
    } catch (e) {
      setErr(msg(e));
    } finally {
      setLoading(false);
    }
  }, [dsId, dir]);

  // The root loads immediately; deeper nodes load on first expand.
  useEffect(() => {
    if (depth === 0) fetchEntries();
  }, [depth, fetchEntries]);

  const toggle = async () => {
    if (!expanded && !loaded) await fetchEntries();
    setExpanded((v) => !v);
    onSelectDir(dir);
  };

  const indent = { paddingLeft: depth * 12 };

  return (
    <div>
      <div
        className="list-item"
        style={{ ...indent, cursor: "pointer", background: activePath === dir ? "var(--hover, rgba(127,127,127,0.12))" : undefined }}
        onClick={toggle}
      >
        <span className="mono">
          {expanded ? "▾ " : "▸ "}📁 {name}
        </span>
      </div>
      {expanded && (
        <>
          {loading && <div className="muted" style={{ ...indent, paddingLeft: depth * 12 + 16 }}>loading…</div>}
          {err && <div style={{ ...indent, color: "var(--red)" }}>{err}</div>}
          {entries.map((e) =>
            e.is_dir ? (
              <TreeNode
                key={e.name}
                dsId={dsId}
                dir={joinPath(dir, e.name)}
                name={e.name}
                depth={depth + 1}
                activePath={activePath}
                onOpenFile={onOpenFile}
                onSelectDir={onSelectDir}
              />
            ) : (
              <div
                key={e.name}
                className="list-item"
                style={{ paddingLeft: (depth + 1) * 12, cursor: "pointer", background: activePath === joinPath(dir, e.name) ? "var(--hover, rgba(127,127,127,0.12))" : undefined }}
                onClick={() => onOpenFile(joinPath(dir, e.name))}
              >
                <span className="mono">📄 {e.name}</span>
              </div>
            )
          )}
          {loaded && entries.length === 0 && (
            <div className="muted" style={{ paddingLeft: (depth + 1) * 12 }}>
              empty
            </div>
          )}
        </>
      )}
    </div>
  );
}

export default function DocuspacePage() {
  const { id } = useParams<{ id: string }>();
  const ds = useDocuspaces((s) => (id ? s.byId[id] : undefined));
  const load = useDocuspaces((s) => s.load);
  const rename = useDocuspaces((s) => s.rename);
  const remove = useDocuspaces((s) => s.remove);
  const navigate = useNavigate();

  const [missing, setMissing] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [nameInput, setNameInput] = useState("");
  const [treeVersion, setTreeVersion] = useState(0); // bump to remount/refresh tree
  const [activeDir, setActiveDir] = useState(""); // where "new file/folder" land
  const [file, setFile] = useState<OpenFile | null>(null);
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [editing, setEditing] = useState(false); // markdown edit vs preview
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    load(id).catch(() => setMissing(true));
  }, [id, load]);

  const refreshTree = () => setTreeVersion((v) => v + 1);

  const openFile = async (path: string) => {
    if (!id) return;
    setErr(null);
    try {
      const r = await api.dsRead(id, path);
      if (r.kind === "file") {
        setFile({ path, binary: r.binary, size: r.size, content: r.content || "" });
        setContent(r.binary ? "" : r.content || "");
        setDirty(false);
        setEditing(!isMarkdown(path)); // non-markdown opens straight in the editor
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
      await api.dsWrite(id, file.path, content);
      setFile({ ...file, content });
      setDirty(false);
    } catch (e) {
      setErr(msg(e));
    } finally {
      setSaving(false);
    }
  };

  const newFile = async () => {
    if (!id) return;
    const rel = window.prompt("New file path (e.g. notes/readme.md):", activeDir ? `${activeDir}/` : "");
    if (!rel || !rel.trim()) return;
    const path = rel.trim().replace(/\/+$/, "");
    try {
      await api.dsWrite(id, path, "");
      refreshTree();
      openFile(path);
    } catch (e) {
      setErr(msg(e));
    }
  };

  const newFolder = async () => {
    if (!id) return;
    const rel = window.prompt("New folder path (e.g. notes/drafts):", activeDir ? `${activeDir}/` : "");
    if (!rel || !rel.trim()) return;
    try {
      await api.dsMkdir(id, rel.trim().replace(/\/+$/, ""));
      refreshTree();
    } catch (e) {
      setErr(msg(e));
    }
  };

  const deleteFile = async () => {
    if (!id || !file) return;
    if (!window.confirm(`Delete ${file.path}?`)) return;
    try {
      await api.dsDeletePath(id, file.path);
      setFile(null);
      refreshTree();
    } catch (e) {
      setErr(msg(e));
    }
  };

  const download = () => {
    if (!file) return;
    // Binary content arrives base64; text is inline. Build a data URL either way.
    const href = file.binary
      ? `data:application/octet-stream;base64,${file.content}`
      : `data:text/plain;charset=utf-8,${encodeURIComponent(content)}`;
    const a = document.createElement("a");
    a.href = href;
    a.download = file.path.split("/").pop() || "file";
    a.click();
  };

  const saveName = async () => {
    if (!id) return;
    const n = nameInput.trim();
    if (!n) {
      setRenaming(false);
      return;
    }
    try {
      await rename(id, n);
      setRenaming(false);
    } catch (e) {
      setErr(msg(e));
    }
  };

  const destroy = async () => {
    if (!id || !ds) return;
    if (!window.confirm("Delete this docuspace and all of its files?")) return;
    try {
      await remove(id, ds.project_id);
      navigate(`/projects/${ds.project_id}`);
    } catch (e) {
      setErr(msg(e));
    }
  };

  if (missing) {
    return (
      <div>
        <p className="muted">Docuspace not found.</p>
        <Link to="/">← Projects</Link>
      </div>
    );
  }
  if (!ds) return <p className="muted">Loading…</p>;

  return (
    <div>
      <div className="row" style={{ marginBottom: 8 }}>
        <Link to={`/projects/${ds.project_id}`}>← Back to project</Link>
      </div>

      <div className="card">
        <div className="row" style={{ justifyContent: "space-between" }}>
          {renaming ? (
            <div className="row">
              <input
                value={nameInput}
                autoFocus
                onChange={(e) => setNameInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") saveName();
                  if (e.key === "Escape") setRenaming(false);
                }}
              />
              <button onClick={saveName}>Save</button>
              <button className="secondary" onClick={() => setRenaming(false)}>
                Cancel
              </button>
            </div>
          ) : (
            <h2 style={{ margin: 0 }}>
              {ds.name} <span className="badge">docuspace</span>
            </h2>
          )}
          <div className="row">
            {!renaming && (
              <button
                className="secondary"
                onClick={() => {
                  setNameInput(ds.name);
                  setRenaming(true);
                }}
              >
                Rename
              </button>
            )}
            <button className="secondary" onClick={destroy}>
              Delete
            </button>
          </div>
        </div>
      </div>

      {err && <p style={{ color: "var(--red)" }}>{err}</p>}

      <div style={{ display: "grid", gridTemplateColumns: "minmax(220px, 1fr) 2fr", gap: 12 }}>
        {/* file tree */}
        <div className="card">
          <div className="row" style={{ justifyContent: "space-between", marginBottom: 8 }}>
            <strong>Files</strong>
            <div className="row">
              <button className="secondary" onClick={newFile} title="New file">
                + File
              </button>
              <button className="secondary" onClick={newFolder} title="New folder">
                + Folder
              </button>
              <button className="secondary" onClick={refreshTree} title="Refresh">
                ⟳
              </button>
            </div>
          </div>
          {activeDir && (
            <div className="muted mono" style={{ marginBottom: 4 }}>
              new items → /{activeDir}
            </div>
          )}
          <TreeNode
            key={treeVersion}
            dsId={ds.id}
            dir=""
            name="/"
            depth={0}
            activePath={file?.path ?? ""}
            onOpenFile={openFile}
            onSelectDir={setActiveDir}
          />
        </div>

        {/* viewer / editor */}
        <div className="card">
          {!file ? (
            <p className="muted">Select a file to view or edit, or create one with “+ File”.</p>
          ) : (
            <>
              <div className="row" style={{ justifyContent: "space-between", marginBottom: 8 }}>
                <strong className="mono">{file.path}</strong>
                <div className="row">
                  {!file.binary && isMarkdown(file.path) && (
                    <button className="secondary" onClick={() => setEditing((v) => !v)}>
                      {editing ? "Preview" : "Edit"}
                    </button>
                  )}
                  {!file.binary && (
                    <button onClick={save} disabled={!dirty || saving}>
                      {saving ? "Saving…" : "Save"}
                    </button>
                  )}
                  <button className="secondary" onClick={download}>
                    Download
                  </button>
                  <button className="secondary" onClick={deleteFile}>
                    Delete
                  </button>
                </div>
              </div>

              {file.binary ? (
                <p className="muted">
                  Binary file ({file.size} bytes) — use Download to retrieve it.
                </p>
              ) : isMarkdown(file.path) && !editing ? (
                <div className="md-body">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
                </div>
              ) : (
                <textarea
                  className="mono"
                  style={{ width: "100%", minHeight: 420 }}
                  value={content}
                  spellCheck={false}
                  onChange={(ev) => {
                    setContent(ev.target.value);
                    setDirty(true);
                  }}
                />
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
