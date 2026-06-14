import { useEffect, useState } from "react";
import { Link, Navigate, Route, Routes, useNavigate } from "react-router-dom";
import { api, User } from "./api";
import Login from "./pages/Login";
import ProjectsPage from "./pages/ProjectsPage";
import ProjectPage from "./pages/ProjectPage";
import ApiKeysPage from "./pages/ApiKeysPage";
import DocsPage from "./pages/DocsPage";

export default function App() {
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api
      .me()
      .then(setUser)
      .catch(() => setUser(null))
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return <div className="container">Loading…</div>;
  }

  if (!user) {
    return <Login />;
  }

  return (
    <>
      <Nav user={user} onLogout={() => setUser(null)} />
      <div className="container">
        <Routes>
          <Route path="/" element={<ProjectsPage />} />
          <Route path="/projects/:id" element={<ProjectPage />} />
          <Route path="/keys" element={<ApiKeysPage />} />
          <Route path="/docs" element={<DocsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </div>
    </>
  );
}

function Nav({ user, onLogout }: { user: User; onLogout: () => void }) {
  const navigate = useNavigate();
  const logout = async () => {
    await api.logout();
    onLogout();
    navigate("/");
  };
  return (
    <div className="nav">
      <span className="brand">🛠️ Sprite Builder</span>
      <Link to="/">Projects</Link>
      <Link to="/keys">API Keys</Link>
      <Link to="/docs">Docs</Link>
      <span className="spacer" />
      {user.avatar_url && <img className="avatar" src={user.avatar_url} alt="" />}
      <span className="muted">{user.github_login}</span>
      <button className="secondary" onClick={logout}>
        Logout
      </button>
    </div>
  );
}
