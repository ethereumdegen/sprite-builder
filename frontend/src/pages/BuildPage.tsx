// Dedicated build show page (/builds/:id): full-page diagnostics + live logs for
// a single build. Reachable from the project build list and the admin dashboard.
import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { useBuilds } from "../stores/builds";
import { BuildBody, isActive } from "../components/build";

export default function BuildPage() {
  const { id } = useParams<{ id: string }>();
  const build = useBuilds((s) => (id ? s.byId[id] : undefined));
  const loadBuild = useBuilds((s) => s.loadBuild);
  const [now, setNow] = useState(Date.now());
  const [missing, setMissing] = useState(false);

  // Poll while loading and while the build is in flight, keeping a live clock
  // for durations. Stops polling once the build reaches a terminal state.
  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    const tick = () => {
      setNow(Date.now());
      loadBuild(id).catch(() => {
        if (!cancelled) setMissing(true);
      });
    };
    tick();
    const t = setInterval(tick, 1500);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, [id, loadBuild]);

  if (missing) {
    return (
      <div>
        <p className="muted">Build not found.</p>
        <Link to="/">← Projects</Link>
      </div>
    );
  }

  if (!build) {
    return <p className="muted">Loading…</p>;
  }

  const live = isActive(build);

  return (
    <div>
      <div className="row" style={{ marginBottom: 8 }}>
        <Link to={`/projects/${build.project_id}`}>← Back to project</Link>
      </div>

      <div className="card">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h2 style={{ margin: 0 }}>
            Build <span className="mono">{build.id.slice(0, 8)}</span>{" "}
            <span className={"badge " + build.status}>{build.status}</span>{" "}
            {live && <span className="spin">⟳</span>}
          </h2>
        </div>

        <BuildBody build={build} now={now} />
      </div>
    </div>
  );
}
