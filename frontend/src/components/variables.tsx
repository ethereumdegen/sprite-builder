// Per-project environment variables editor (Railway-style). Values are injected
// into the deployed container's `docker run` by the worker; they apply on the
// next build. Re-adding an existing name overwrites it (upsert).
import { useEffect, useState } from "react";
import { useEnvVars } from "../stores/env";

// Mirror the backend's `valid_env_key`.
const validKey = (k: string) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(k);

export function VariablesEditor({ projectId }: { projectId: string }) {
  const byProject = useEnvVars((s) => s.byProject);
  const load = useEnvVars((s) => s.load);
  const upsert = useEnvVars((s) => s.upsert);
  const remove = useEnvVars((s) => s.remove);
  const vars = byProject[projectId] || [];

  const [newKey, setNewKey] = useState("");
  const [newValue, setNewValue] = useState("");
  const [reveal, setReveal] = useState<Record<string, boolean>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    load(projectId).catch(() => {});
  }, [projectId, load]);

  const add = async () => {
    const key = newKey.trim();
    if (!validKey(key)) {
      setError("Invalid name — use letters, digits, underscores; can't start with a digit.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await upsert(projectId, key, newValue);
      setNewKey("");
      setNewValue("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card">
      <h3 style={{ marginTop: 0 }}>Variables</h3>
      <p className="muted">
        Injected into the container at run time. Changes apply on the next build — trigger a
        build to redeploy.
      </p>
      {error && <p style={{ color: "var(--red)" }}>{error}</p>}

      {vars.length > 0 && (
        <div style={{ marginBottom: 12 }}>
          {vars.map((v) => (
            <div className="list-item" key={v.id}>
              <div className="mono" style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>
                <b>{v.key}</b>
                <span className="muted"> = </span>
                <span>{reveal[v.key] ? v.value : "••••••••"}</span>
              </div>
              <div className="row">
                <button
                  className="secondary"
                  onClick={() => setReveal((r) => ({ ...r, [v.key]: !r[v.key] }))}
                >
                  {reveal[v.key] ? "Hide" : "Show"}
                </button>
                <button
                  className="secondary"
                  onClick={() =>
                    remove(projectId, v.key).catch((e) =>
                      setError(e instanceof Error ? e.message : String(e))
                    )
                  }
                >
                  Delete
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="grid2">
        <div>
          <label>Name</label>
          <input
            placeholder="EXTERNAL_API_KEY"
            value={newKey}
            onChange={(e) => setNewKey(e.target.value)}
          />
        </div>
        <div>
          <label>Value</label>
          <input value={newValue} onChange={(e) => setNewValue(e.target.value)} />
        </div>
      </div>
      <div style={{ marginTop: 12 }}>
        <button onClick={add} disabled={busy || !newKey.trim()}>
          {busy ? "Saving…" : "Add variable"}
        </button>
      </div>
    </div>
  );
}
