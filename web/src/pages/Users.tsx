import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import { Card } from "../components/ui";

type User = { id: string; username: string; email: string; created_at: string };

export default function Users() {
  const { data, isLoading, error } = useQuery({
    queryKey: ["users"],
    queryFn: async () => {
      const { data, error } = await api.GET("/api/v1/users", { params: { query: {} } });
      if (error) throw error;
      return data;
    },
  });

  const items = (data as { items?: User[] } | undefined)?.items ?? [];

  return (
    <div className="space-y-4">
      <h1 className="text-2xl font-semibold">Users</h1>
      {isLoading ? (
        <p className="text-slate-500">Loading…</p>
      ) : error ? (
        <p className="text-red-600 text-sm">Failed to load users (need users.manage_all).</p>
      ) : (
        <Card className="p-0 overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-slate-50 text-slate-600">
              <tr>
                <th className="text-left px-4 py-2">Username</th>
                <th className="text-left px-4 py-2">Email</th>
                <th className="text-left px-4 py-2">Created</th>
              </tr>
            </thead>
            <tbody>
              {items.map((u) => (
                <tr key={u.id} className="border-t border-slate-200">
                  <td className="px-4 py-2 font-medium">{u.username}</td>
                  <td className="px-4 py-2">{u.email}</td>
                  <td className="px-4 py-2 text-slate-600">{new Date(u.created_at).toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
    </div>
  );
}
