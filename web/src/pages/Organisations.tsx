import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import { Button, Card, ErrorBanner, Field, Input } from "../components/ui";

export default function Organisations() {
  const qc = useQueryClient();
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [business, setBusiness] = useState("");
  const [error, setError] = useState<string | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ["my-organisations"],
    queryFn: async () => {
      const { data, error } = await api.GET("/api/v1/tenants/me/organisations", { params: { query: {} } });
      if (error) throw error;
      return data;
    },
  });

  const createMut = useMutation({
    mutationFn: async () => {
      const { data, error } = await api.POST("/api/v1/tenants/organisations", { body: { name, business } });
      if (error) throw error;
      return data;
    },
    onSuccess: () => {
      setName(""); setBusiness(""); setCreating(false);
      qc.invalidateQueries({ queryKey: ["my-organisations"] });
    },
    onError: (e: unknown) => setError((e as { detail?: string })?.detail ?? "Create failed"),
  });

  const items = (data as { items?: Array<{ id: string; name: string; business: string; role_codes: string[] }> } | undefined)?.items ?? [];

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Organisations</h1>
        <Button onClick={() => setCreating((v) => !v)}>{creating ? "Cancel" : "+ New"}</Button>
      </div>

      {creating && (
        <Card>
          <form
            onSubmit={(e) => { e.preventDefault(); setError(null); createMut.mutate(); }}
            className="space-y-3"
          >
            <Field label="Name"><Input value={name} onChange={(e) => setName(e.target.value)} required /></Field>
            <Field label="Business"><Input value={business} onChange={(e) => setBusiness(e.target.value)} required /></Field>
            <ErrorBanner message={error} />
            <Button type="submit" disabled={createMut.isPending}>Create</Button>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-slate-500">Loading…</p>
      ) : items.length === 0 ? (
        <p className="text-slate-500">You don't belong to any organisation yet.</p>
      ) : (
        <Card className="p-0 overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-slate-600">
              <tr>
                <th className="text-left px-4 py-2">Name</th>
                <th className="text-left px-4 py-2">Business</th>
                <th className="text-left px-4 py-2">Roles</th>
                <th className="text-right px-4 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {items.map((o) => (
                <tr key={o.id} className="border-t border-slate-200">
                  <td className="px-4 py-2 font-medium">{o.name}</td>
                  <td className="px-4 py-2">{o.business}</td>
                  <td className="px-4 py-2 text-slate-600">{o.role_codes.join(", ")}</td>
                  <td className="px-4 py-2 text-right">
                    <Link to={`/organisations/${o.id}/members`} className="text-slate-700 hover:underline mr-4">Members</Link>
                    <Link to={`/organisations/${o.id}/channels`} className="text-slate-700 hover:underline">Channels</Link>
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
