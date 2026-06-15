import { useEffect, useState } from "react";
import { useKeys } from "../stores/keys";

export default function ApiKeysPage() {
  const { keys, load, create, remove } = useKeys();
  const [name, setName] = useState("");
  const [newSecret, setNewSecret] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    load();
  }, [load]);

  const onCreate = async () => {
    if (!name.trim()) return;
    setCreating(true);
    try {
      const res = await create(name.trim());
      setNewSecret(res.secret);
      setName("");
    } finally {
      setCreating(false);
    }
  };

  const onRemove = async (id: string) => {
    if (!confirm("Revoke this key? Any client using it will stop working.")) return;
    await remove(id);
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
          <button onClick={onCreate} disabled={creating} style={{ whiteSpace: "nowrap" }}>
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
                <button className="danger" onClick={() => onRemove(k.id)}>
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
