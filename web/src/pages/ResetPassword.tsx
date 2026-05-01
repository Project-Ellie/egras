import { useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import { Button, Card, ErrorBanner, Field, Input } from "../components/ui";

export default function ResetPassword() {
  const [stage, setStage] = useState<"request" | "confirm">("request");
  const [email, setEmail] = useState("");
  const [token, setToken] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  async function onRequest(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    const { error: err } = await api.POST("/api/v1/security/password-reset-request", {
      body: { email },
    });
    if (err) setError("Request failed");
    else setStage("confirm");
  }

  async function onConfirm(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    const { error: err } = await api.POST("/api/v1/security/password-reset-confirm", {
      body: { token, new_password: newPassword },
    });
    if (err) setError((err as { detail?: string })?.detail ?? "Reset failed");
    else setDone(true);
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-slate-50 px-4">
      <Card className="w-full max-w-sm">
        <h1 className="text-xl font-semibold mb-4">Reset password</h1>
        {done ? (
          <div className="space-y-3">
            <p className="text-sm">Password updated.</p>
            <Link to="/login" className="text-sm text-slate-700 hover:underline">Back to login</Link>
          </div>
        ) : stage === "request" ? (
          <form onSubmit={onRequest} className="space-y-3">
            <Field label="Email"><Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required /></Field>
            <ErrorBanner message={error} />
            <Button type="submit" className="w-full">Send reset email</Button>
            <button
              type="button"
              className="text-sm text-slate-600 hover:underline"
              onClick={() => setStage("confirm")}
            >
              I already have a token
            </button>
          </form>
        ) : (
          <form onSubmit={onConfirm} className="space-y-3">
            <Field label="Reset token"><Input value={token} onChange={(e) => setToken(e.target.value)} required /></Field>
            <Field label="New password"><Input type="password" value={newPassword} onChange={(e) => setNewPassword(e.target.value)} required /></Field>
            <ErrorBanner message={error} />
            <Button type="submit" className="w-full">Set new password</Button>
          </form>
        )}
      </Card>
    </div>
  );
}
