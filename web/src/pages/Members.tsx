import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "react-router-dom";
import { api } from "../api/client";
import { Button, Card, ErrorBanner, Field, Input } from "../components/ui";

type Member = { user_id: string; username: string; email: string; role_codes: string[] };

export default function Members() {
  const { orgId = "" } = useParams<{ orgId: string }>();
  const qc = useQueryClient();
  const [adding, setAdding] = useState(false);
  const [userId, setUserId] = useState("");
  const [roleCode, setRoleCode] = useState("org_member");
  const [error, setError] = useState<string | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ["members", orgId],
    queryFn: async () => {
      const { data, error } = await api.GET("/api/v1/tenants/organisations/{id}/members", {
        params: { path: { id: orgId }, query: {} },
      });
      if (error) throw error;
      return data;
    },
  });

  const addMut = useMutation({
    mutationFn: async () => {
      const { error } = await api.POST("/api/v1/tenants/add-user-to-organisation", {
        body: { org_id: orgId, user_id: userId, role_code: roleCode },
      });
      if (error) throw error;
    },
    onSuccess: () => {
      setUserId(""); setAdding(false); setError(null);
      qc.invalidateQueries({ queryKey: ["members", orgId] });
    },
    onError: (e: unknown) => setError((e as { detail?: string })?.detail ?? "Add failed"),
  });

  const removeMut = useMutation({
    mutationFn: async (uid: string) => {
      const { error } = await api.POST("/api/v1/tenants/remove-user-from-organisation", {
        body: { org_id: orgId, user_id: uid },
      });
      if (error) throw error;
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["members", orgId] }),
  });

  const items = (data as { items?: Member[] } | undefined)?.items ?? [];

  return (
    <div className="space-y-4">
      <h1 className="text-2xl font-semibold">Members</h1>
      <p className="text-sm text-slate-500 break-all">Org: {orgId}</p>

      <div className="flex justify-end">
        <Button onClick={() => setAdding((v) => !v)}>{adding ? "Cancel" : "+ Add member"}</Button>
      </div>
      {adding && (
        <Card>
          <form
            onSubmit={(e) => { e.preventDefault(); setError(null); addMut.mutate(); }}
            className="space-y-3"
          >
            <Field label="User ID"><Input value={userId} onChange={(e) => setUserId(e.target.value)} required /></Field>
            <Field label="Role code">
              <Input value={roleCode} onChange={(e) => setRoleCode(e.target.value)} required />
            </Field>
            <ErrorBanner message={error} />
            <Button type="submit" disabled={addMut.isPending}>Add</Button>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-slate-500">Loading…</p>
      ) : (
        <Card className="p-0 overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-slate-600">
              <tr>
                <th className="text-left px-4 py-2">Username</th>
                <th className="text-left px-4 py-2">Email</th>
                <th className="text-left px-4 py-2">Roles</th>
                <th className="text-right px-4 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {items.map((m) => (
                <tr key={m.user_id} className="border-t border-slate-200">
                  <td className="px-4 py-2 font-medium">{m.username}</td>
                  <td className="px-4 py-2">{m.email}</td>
                  <td className="px-4 py-2 text-slate-600">{m.role_codes.join(", ")}</td>
                  <td className="px-4 py-2 text-right">
                    <button
                      onClick={() => removeMut.mutate(m.user_id)}
                      className="text-red-600 hover:underline"
                    >
                      Remove
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
    </div>
  );
}
