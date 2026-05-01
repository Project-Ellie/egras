import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api } from "../api/client";
import { useAuth } from "../auth/AuthContext";
import { Button, Card, ErrorBanner, Field, Input } from "../components/ui";

export default function Login() {
  const navigate = useNavigate();
  const { login } = useAuth();
  const [usernameOrEmail, setUsernameOrEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    const { data, error: err } = await api.POST("/api/v1/security/login", {
      body: { username_or_email: usernameOrEmail, password },
    });
    setBusy(false);
    if (err || !data) {
      setError((err as { detail?: string })?.detail ?? "Login failed");
      return;
    }
    login(data.token);
    navigate("/");
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-slate-50 px-4">
      <Card className="w-full max-w-sm">
        <h1 className="text-xl font-semibold mb-4">Sign in to egras</h1>
        <form onSubmit={onSubmit} className="space-y-3">
          <Field label="Username or email">
            <Input
              autoFocus
              value={usernameOrEmail}
              onChange={(e) => setUsernameOrEmail(e.target.value)}
              required
            />
          </Field>
          <Field label="Password">
            <Input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
          </Field>
          <ErrorBanner message={error} />
          <Button type="submit" disabled={busy} className="w-full">
            {busy ? "Signing in…" : "Sign in"}
          </Button>
          <div className="flex justify-between text-sm text-slate-600 pt-1">
            <Link to="/register" className="hover:underline">Register</Link>
            <Link to="/reset-password" className="hover:underline">Forgot password?</Link>
          </div>
        </form>
      </Card>
    </div>
  );
}
