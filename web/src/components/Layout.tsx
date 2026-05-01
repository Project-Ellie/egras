import { Link, NavLink, Outlet, useNavigate } from "react-router-dom";
import { useAuth } from "../auth/AuthContext";
import { api } from "../api/client";

const navCls = ({ isActive }: { isActive: boolean }) =>
  `px-3 py-2 rounded ${isActive ? "bg-slate-900 text-white" : "text-slate-700 hover:bg-slate-200"}`;

export default function Layout() {
  const { claims, logout } = useAuth();
  const navigate = useNavigate();

  async function handleLogout() {
    await api.POST("/api/v1/security/logout", {});
    logout();
    navigate("/login");
  }

  return (
    <div className="min-h-screen flex flex-col">
      <header className="bg-white border-b border-slate-200">
        <div className="max-w-6xl mx-auto px-4 py-3 flex items-center gap-4">
          <Link to="/" className="font-semibold text-lg">egras</Link>
          <nav className="flex gap-1 flex-1">
            <NavLink to="/organisations" className={navCls}>Organisations</NavLink>
            <NavLink to="/users" className={navCls}>Users</NavLink>
          </nav>
          <span className="text-sm text-slate-600">{claims?.username ?? claims?.sub}</span>
          <button
            onClick={handleLogout}
            className="px-3 py-1.5 text-sm rounded border border-slate-300 hover:bg-slate-100"
          >
            Logout
          </button>
        </div>
      </header>
      <main className="flex-1 max-w-6xl w-full mx-auto px-4 py-6">
        <Outlet />
      </main>
    </div>
  );
}
