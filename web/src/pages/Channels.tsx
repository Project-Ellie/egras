import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "react-router-dom";
import { api } from "../api/client";
import { Button, Card, ErrorBanner, Field, Input } from "../components/ui";

const CHANNEL_TYPES = ["vast", "sensor", "websocket", "rest"] as const;
type ChannelType = (typeof CHANNEL_TYPES)[number];

type Channel = {
  id: string;
  name: string;
  description: string | null;
  channel_type: ChannelType;
  api_key: string;
  is_active: boolean;
};

export default function Channels() {
  const { orgId = "" } = useParams<{ orgId: string }>();
  const qc = useQueryClient();
  const [creating, setCreating] = useState(false);
  const [form, setForm] = useState<{ name: string; description: string; channel_type: ChannelType; is_active: boolean }>({
    name: "",
    description: "",
    channel_type: "rest",
    is_active: true,
  });
  const [error, setError] = useState<string | null>(null);
  const [revealedKey, setRevealedKey] = useState<string | null>(null);

  const { data, isLoading } = useQuery({
    queryKey: ["channels", orgId],
    queryFn: async () => {
      const { data, error } = await api.GET("/api/v1/tenants/organisations/{org_id}/channels", {
        params: { path: { org_id: orgId }, query: {} },
      });
      if (error) throw error;
      return data;
    },
  });

  const createMut = useMutation({
    mutationFn: async () => {
      const { data, error } = await api.POST("/api/v1/tenants/organisations/{org_id}/channels", {
        params: { path: { org_id: orgId } },
        body: {
          name: form.name,
          description: form.description || null,
          channel_type: form.channel_type,
          is_active: form.is_active,
        },
      });
      if (error) throw error;
      return data as Channel;
    },
    onSuccess: (ch) => {
      setRevealedKey(ch.api_key);
      setForm({ name: "", description: "", channel_type: "rest", is_active: true });
      setCreating(false);
      qc.invalidateQueries({ queryKey: ["channels", orgId] });
    },
    onError: (e: unknown) => setError((e as { detail?: string })?.detail ?? "Create failed"),
  });

  const toggleMut = useMutation({
    mutationFn: async (ch: Channel) => {
      const { error } = await api.PUT("/api/v1/tenants/organisations/{org_id}/channels/{channel_id}", {
        params: { path: { org_id: orgId, channel_id: ch.id } },
        body: {
          name: ch.name,
          description: ch.description,
          channel_type: ch.channel_type,
          is_active: !ch.is_active,
        },
      });
      if (error) throw error;
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["channels", orgId] }),
  });

  const deleteMut = useMutation({
    mutationFn: async (id: string) => {
      const { error } = await api.DELETE("/api/v1/tenants/organisations/{org_id}/channels/{channel_id}", {
        params: { path: { org_id: orgId, channel_id: id } },
      });
      if (error) throw error;
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["channels", orgId] }),
  });

  const items = (data as { items?: Channel[] } | undefined)?.items ?? [];

  return (
    <div className="space-y-4">
      <h1 className="text-2xl font-semibold">Inbound Channels</h1>
      <p className="text-sm text-slate-500 break-all">Org: {orgId}</p>

      <div className="flex justify-end">
        <Button onClick={() => setCreating((v) => !v)}>{creating ? "Cancel" : "+ New channel"}</Button>
      </div>

      {revealedKey && (
        <Card className="bg-amber-50 border-amber-200">
          <p className="text-sm font-medium text-amber-900">Save this API key — it will not be shown again:</p>
          <code className="block mt-2 break-all text-xs">{revealedKey}</code>
          <button onClick={() => setRevealedKey(null)} className="text-sm text-amber-900 hover:underline mt-2">
            Dismiss
          </button>
        </Card>
      )}

      {creating && (
        <Card>
          <form
            onSubmit={(e) => { e.preventDefault(); setError(null); createMut.mutate(); }}
            className="space-y-3"
          >
            <Field label="Name"><Input value={form.name} onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} required /></Field>
            <Field label="Description">
              <Input value={form.description} onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))} />
            </Field>
            <Field label="Type">
              <select
                value={form.channel_type}
                onChange={(e) => setForm((f) => ({ ...f, channel_type: e.target.value as ChannelType }))}
                className="block w-full px-3 py-1.5 border border-slate-300 rounded text-sm"
              >
                {CHANNEL_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
              </select>
            </Field>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={form.is_active}
                onChange={(e) => setForm((f) => ({ ...f, is_active: e.target.checked }))}
              />
              Active
            </label>
            <ErrorBanner message={error} />
            <Button type="submit" disabled={createMut.isPending}>Create</Button>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-slate-500">Loading…</p>
      ) : items.length === 0 ? (
        <p className="text-slate-500">No channels yet.</p>
      ) : (
        <Card className="p-0 overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-slate-600">
              <tr>
                <th className="text-left px-4 py-2">Name</th>
                <th className="text-left px-4 py-2">Type</th>
                <th className="text-left px-4 py-2">Active</th>
                <th className="text-left px-4 py-2">Description</th>
                <th className="text-right px-4 py-2"></th>
              </tr>
            </thead>
            <tbody>
              {items.map((c) => (
                <tr key={c.id} className="border-t border-slate-200">
                  <td className="px-4 py-2 font-medium">{c.name}</td>
                  <td className="px-4 py-2">{c.channel_type}</td>
                  <td className="px-4 py-2">{c.is_active ? "yes" : "no"}</td>
                  <td className="px-4 py-2 text-slate-600">{c.description ?? "—"}</td>
                  <td className="px-4 py-2 text-right space-x-3">
                    <button onClick={() => toggleMut.mutate(c)} className="text-slate-700 hover:underline">
                      {c.is_active ? "Disable" : "Enable"}
                    </button>
                    <button
                      onClick={() => { if (confirm(`Delete channel "${c.name}"?`)) deleteMut.mutate(c.id); }}
                      className="text-red-600 hover:underline"
                    >
                      Delete
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
