import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { api } from "../api/client";
import { Button, Card, ErrorBanner, Field, Input } from "../components/ui";

export default function Register() {
  const navigate = useNavigate();
  const [form, setForm] = useState({
    username: "",
    email: "",
    password: "",
    org_id: "",
    role_code: "org_member",
  });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function update<K extends keyof typeof form>(k: K) {
    return (e: React.ChangeEvent<HTMLInputElement>) =>
      setForm((f) => ({ ...f, [k]: e.target.value }));
  }

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    const { error: err } = await api.POST("/api/v1/security/register", { body: form });
    setBusy(false);
    if (err) {
      setError((err as { detail?: string })?.detail ?? "Registration failed");
      return;
    }
    navigate("/login");
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-slate-50 px-4">
      <Card className="w-full max-w-sm">
        <h1 className="text-xl font-semibold mb-4">Register</h1>
        <form onSubmit={onSubmit} className="space-y-3">
          <Field label="Username"><Input value={form.username} onChange={update("username")} required /></Field>
          <Field label="Email"><Input type="email" value={form.email} onChange={update("email")} required /></Field>
          <Field label="Password"><Input type="password" value={form.password} onChange={update("password")} required /></Field>
          <Field label="Organisation ID"><Input value={form.org_id} onChange={update("org_id")} required /></Field>
          <Field label="Role code"><Input value={form.role_code} onChange={update("role_code")} required /></Field>
          <ErrorBanner message={error} />
          <Button type="submit" disabled={busy} className="w-full">
            {busy ? "Registering…" : "Register"}
          </Button>
          <div className="text-sm text-slate-600 pt-1">
            <Link to="/login" className="hover:underline">Back to login</Link>
          </div>
        </form>
      </Card>
    </div>
  );
}
