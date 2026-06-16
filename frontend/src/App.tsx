import { useEffect } from "react";
import { Link, Navigate, Route, Routes, useNavigate } from "react-router-dom";
import { User } from "./api";
import { useAuth } from "./stores/auth";
import Login from "./pages/Login";
import ProjectsPage from "./pages/ProjectsPage";
import ProjectPage from "./pages/ProjectPage";
import BuildPage from "./pages/BuildPage";
import CodespacePage from "./pages/CodespacePage";
import DocuspacePage from "./pages/DocuspacePage";
import ApiKeysPage from "./pages/ApiKeysPage";
import DocsPage from "./pages/DocsPage";
import AdminPage from "./pages/AdminPage";

export default function App() {
  const { user, loading, loadMe } = useAuth();

  useEffect(() => {
    loadMe();
  }, [loadMe]);

  if (loading) {
    return <div className="container">Loading…</div>;
  }

  if (!user) {
    return <Login />;
  }

  return (
    <>
      <Nav user={user} />
      <div className="container">
        <Routes>
          <Route path="/" element={<ProjectsPage />} />
          <Route path="/projects/:id" element={<ProjectPage />} />
          <Route path="/builds/:id" element={<BuildPage />} />
          <Route path="/codespaces/:id" element={<CodespacePage />} />
          <Route path="/docuspaces/:id" element={<DocuspacePage />} />
          <Route path="/keys" element={<ApiKeysPage />} />
          <Route path="/docs" element={<DocsPage />} />
          <Route path="/admin" element={<AdminRoute />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </div>
    </>
  );
}

/// Capability-gated route: non-admins are redirected home (ADR 0016).
function AdminRoute() {
  const can = useAuth((s) => s.can);
  return can("view_admin_dashboard") ? <AdminPage /> : <Navigate to="/" replace />;
}

function Nav({ user }: { user: User }) {
  const navigate = useNavigate();
  const logout = useAuth((s) => s.logout);
  const can = useAuth((s) => s.can);
  const onLogout = async () => {
    await logout();
    navigate("/");
  };
  return (
    <div className="nav">
      <span className="brand">🛠️ Sprite Builder</span>
      <Link to="/">Projects</Link>
      <Link to="/keys">API Keys</Link>
      <Link to="/docs">Docs</Link>
      {can("view_admin_dashboard") && <Link to="/admin">Admin</Link>}
      <span className="spacer" />
      {user.avatar_url && <img className="avatar" src={user.avatar_url} alt="" />}
      <span className="muted">{user.github_login}</span>
      <button className="secondary" onClick={onLogout}>
        Logout
      </button>
    </div>
  );
}
