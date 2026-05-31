import { api } from "../api";

export default function Login() {
  return (
    <div className="container" style={{ maxWidth: 460, marginTop: "12vh" }}>
      <div className="card" style={{ textAlign: "center", padding: 32 }}>
        <h1 style={{ marginTop: 0 }}>🛠️ Sprite Builder</h1>
        <p className="muted">
          Connect a GitHub repo, trigger a build, and we'll spin up a{" "}
          <a href="https://sprites.dev" target="_blank" rel="noreferrer">
            sprite
          </a>{" "}
          to build &amp; host it for you.
        </p>
        <a href={api.loginUrl}>
          <button style={{ width: "100%", marginTop: 12 }}>Sign in with GitHub</button>
        </a>
      </div>
    </div>
  );
}
