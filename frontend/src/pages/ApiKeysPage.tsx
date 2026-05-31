import { useEffect, useState } from "react";
import { api, ApiKey } from "../api";

export default function ApiKeysPage() {
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [name, setName] = useState("");
  const [newSecret, setNewSecret] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const load = () => api.keys().then(setKeys);
  useEffect(() => {
    load();
  }, []);

  const create = async () => {
    if (!name.trim()) return;
    setCreating(true);
    try {
      const res = await api.createKey(name.trim());
      setNewSecret(res.secret);
      setName("");
      load();
    } finally {
      setCreating(false);
    }
  };

  const remove = async (id: string) => {
    if (!confirm("Revoke this key? Any client using it will stop working.")) return;
    await api.deleteKey(id);
    load();
  };

  return (
    <div>
      <h2>API Keys</h2>
      <p className="muted">
        Use a key as a bearer token to call the API programmatically:
        <br />
        <span className="mono">
          curl -H "Authorization: Bearer &lt;key&gt;" {location.origin}/api/projects
        </span>
      </p>

      <div className="card">
        <label>Create a new key</label>
        <div className="row">
          <input
            placeholder="key name (e.g. CI deploy)"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <button onClick={create} disabled={creating} style={{ whiteSpace: "nowrap" }}>
            {creating ? "Creating…" : "Create"}
          </button>
        </div>

        {newSecret && (
          <div style={{ marginTop: 12 }}>
            <label style={{ color: "var(--green)" }}>
              Copy this now — it won't be shown again:
            </label>
            <div className="secret-box mono">{newSecret}</div>
          </div>
        )}
      </div>

      {keys.length === 0 ? (
        <p className="muted">No keys yet.</p>
      ) : (
        <div className="card">
          {keys.map((k) => (
            <div className="list-item" key={k.id}>
              <div>
                <strong>{k.name}</strong>
                <div className="muted mono">{k.key_prefix}…</div>
              </div>
              <div className="row">
                <span className="muted">
                  {k.last_used_at
                    ? `last used ${new Date(k.last_used_at).toLocaleDateString()}`
                    : "never used"}
                </span>
                <button className="danger" onClick={() => remove(k.id)}>
                  Revoke
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
