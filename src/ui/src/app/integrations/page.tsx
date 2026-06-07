"use client";

import { useEffect, useMemo, useState } from "react";
import { Search, Check, Plus, Puzzle, Trash2 } from "lucide-react";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { IntegrationDialog } from "@/components/integration-dialog";
import { BrandIcon } from "@/components/brand-icons";
import { createMcpServer, deleteMcpServer, listIntegrationKeys, listMcpServers } from "@/lib/api";
import {
  integrationsByCategory,
  type Integration,
} from "@/lib/integrations";
import type { McpServer } from "@/lib/types";

export default function IntegrationsPage() {
  const [connected, setConnected] = useState<Set<string>>(new Set());
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState<Integration | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [mcpForm, setMcpForm] = useState({
    name: "",
    url: "",
    auth_type: "api_key",
    auth_value: "",
    description: "",
  });
  const [mcpError, setMcpError] = useState<string | null>(null);
  const [savingMcp, setSavingMcp] = useState(false);

  const refresh = async () => {
    try {
      const [keys, servers] = await Promise.all([listIntegrationKeys(), listMcpServers()]);
      setConnected(new Set(keys));
      setMcpServers(servers);
      setMcpError(null);
    } catch (e) {
      setConnected(new Set());
      setMcpServers([]);
      setMcpError(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    return integrationsByCategory()
      .map(([cat, items]) => {
        const filtered = q
          ? items.filter(
              (it) =>
                it.name.toLowerCase().includes(q) ||
                it.description.toLowerCase().includes(q),
            )
          : items;
        return [cat, filtered] as [string, Integration[]];
      })
      .filter(([, items]) => items.length > 0);
  }, [query]);

  const openDialog = (it: Integration) => {
    setActive(it);
    setDialogOpen(true);
  };

  const saveMcpServer = async () => {
    setSavingMcp(true);
    setMcpError(null);
    try {
      if (!mcpForm.name.trim() || !mcpForm.url.trim()) {
        throw new Error("Name and URL are required");
      }
      await createMcpServer({
        name: mcpForm.name.trim(),
        url: mcpForm.url.trim(),
        auth_type: mcpForm.auth_type,
        auth_value: mcpForm.auth_type === "none" ? undefined : mcpForm.auth_value,
        description: mcpForm.description.trim() || undefined,
      });
      setMcpForm({
        name: "",
        url: "",
        auth_type: "api_key",
        auth_value: "",
        description: "",
      });
      await refresh();
    } catch (e) {
      setMcpError(e instanceof Error ? e.message : String(e));
    } finally {
      setSavingMcp(false);
    }
  };

  const removeMcpServer = async (server: McpServer) => {
    if (!confirm(`Delete MCP server "${server.name}"?`)) return;
    setMcpServers((prev) => prev.filter((item) => item.id !== server.id));
    try {
      await deleteMcpServer(server.id);
    } catch (e) {
      setMcpError(e instanceof Error ? e.message : String(e));
      refresh();
    }
  };

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar />
      <div className="flex flex-1 flex-col min-w-0">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
          <div className="flex items-center gap-2">
            <Puzzle className="size-4" />
            <span className="text-sm font-semibold">Integrations</span>
          </div>
          <ThemeToggle />
        </header>

        <main className="flex-1 overflow-y-auto">
          <div className="mx-auto w-full max-w-4xl px-6 py-6">
            <div className="mb-6">
              <h1 className="text-lg font-semibold">MCP Gateway</h1>
              <p className="text-sm text-muted-foreground">
                Add streamable HTTP MCP servers here, then attach them from an agent.
              </p>
            </div>

            <section className="mb-8 grid gap-3">
              <div className="flex items-center justify-between gap-3">
                <h2 className="text-sm font-semibold">Streamable HTTP</h2>
                <span className="text-xs text-muted-foreground">{mcpServers.length} configured</span>
              </div>
              <div className="grid gap-3 rounded-lg border border-border bg-card p-4">
                <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
                  <div className="grid gap-1.5">
                    <Label htmlFor="mcp-name">Name</Label>
                    <Input
                      id="mcp-name"
                      value={mcpForm.name}
                      onChange={(event) => setMcpForm({ ...mcpForm, name: event.target.value })}
                      placeholder="linear"
                    />
                  </div>
                  <div className="grid gap-1.5">
                    <Label htmlFor="mcp-url">URL</Label>
                    <Input
                      id="mcp-url"
                      value={mcpForm.url}
                      onChange={(event) => setMcpForm({ ...mcpForm, url: event.target.value })}
                      placeholder="https://mcp.example.com/mcp"
                      className="font-mono text-xs"
                    />
                  </div>
                </div>
                <div className="grid gap-3 md:grid-cols-[180px_minmax(0,1fr)_auto] md:items-end">
                  <div className="grid gap-1.5">
                    <Label htmlFor="mcp-auth-type">Key type</Label>
                    <select
                      id="mcp-auth-type"
                      value={mcpForm.auth_type}
                      onChange={(event) => setMcpForm({ ...mcpForm, auth_type: event.target.value })}
                      className="h-9 rounded-md border border-input bg-background px-3 text-sm"
                    >
                      <option value="api_key">x-api-key</option>
                      <option value="bearer_token">Bearer token</option>
                      <option value="authorization">Authorization</option>
                      <option value="none">No key</option>
                    </select>
                  </div>
                  <div className="grid gap-1.5">
                    <Label htmlFor="mcp-key">Key</Label>
                    <Input
                      id="mcp-key"
                      type="password"
                      value={mcpForm.auth_value}
                      onChange={(event) => setMcpForm({ ...mcpForm, auth_value: event.target.value })}
                      placeholder={mcpForm.auth_type === "none" ? "Not required" : "MCP server key"}
                      disabled={mcpForm.auth_type === "none"}
                      className="font-mono text-xs"
                    />
                  </div>
                  <Button onClick={saveMcpServer} disabled={savingMcp}>
                    <Plus className="size-3.5" />
                    {savingMcp ? "Adding..." : "Add"}
                  </Button>
                </div>
                <div className="grid gap-1.5">
                  <Label htmlFor="mcp-description">Description</Label>
                  <Input
                    id="mcp-description"
                    value={mcpForm.description}
                    onChange={(event) => setMcpForm({ ...mcpForm, description: event.target.value })}
                    placeholder="What this MCP server gives agents access to"
                  />
                </div>
                {mcpError && <p className="text-sm text-destructive">{mcpError}</p>}
              </div>

              <div className="grid gap-2">
                {mcpServers.map((server) => (
                  <div
                    key={server.id}
                    className="grid gap-3 rounded-lg border border-border bg-card p-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center"
                  >
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-medium">{server.name}</span>
                        <span className="rounded border border-border px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
                          {server.auth_type}
                        </span>
                      </div>
                      {server.description && (
                        <p className="mt-1 text-xs text-muted-foreground">{server.description}</p>
                      )}
                      <p className="mt-1 break-all font-mono text-[11px] text-muted-foreground">
                        {server.url}
                      </p>
                    </div>
                    <Button variant="ghost" size="icon" onClick={() => removeMcpServer(server)} aria-label={`Delete ${server.name}`}>
                      <Trash2 className="size-4" />
                    </Button>
                  </div>
                ))}
                {mcpServers.length === 0 && (
                  <div className="rounded-lg border border-dashed border-border py-8 text-center text-sm text-muted-foreground">
                    No streamable HTTP MCP servers configured.
                  </div>
                )}
              </div>
            </section>

            <div className="relative mb-6 max-w-sm">
              <Search className="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search…"
                className="h-9 pl-8"
              />
            </div>

            {groups.length === 0 && (
              <div className="py-12 text-center text-sm text-muted-foreground">
                No integrations match “{query}”.
              </div>
            )}

            <div className="space-y-8">
              {groups.map(([cat, items]) => (
                <section key={cat}>
                  <div className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                    {cat}
                  </div>
                  <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                    {items.map((it) => {
                      const isConnected = connected.has(it.envKey);
                      return (
                        <div
                          key={it.id}
                          className="flex items-start gap-3 rounded-xl border border-border bg-card p-4 transition-colors hover:border-foreground/20"
                        >
                          <div className="flex size-9 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-border bg-muted/40">
                            <BrandIcon id={it.id} className="size-5" />
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="font-medium leading-none">{it.name}</div>
                            <p className="mt-1.5 line-clamp-2 text-xs text-muted-foreground">
                              {it.description}
                            </p>
                          </div>
                          <Button
                            size="sm"
                            variant={isConnected ? "secondary" : "outline"}
                            onClick={() => openDialog(it)}
                          >
                            {isConnected ? (
                              <>
                                <Check className="size-3.5" />
                                Connected
                              </>
                            ) : (
                              "Connect"
                            )}
                          </Button>
                        </div>
                      );
                    })}
                  </div>
                </section>
              ))}
            </div>
          </div>
        </main>
      </div>

      <IntegrationDialog
        integration={active}
        open={dialogOpen}
        connected={active ? connected.has(active.envKey) : false}
        onOpenChange={setDialogOpen}
        onChange={refresh}
      />
    </div>
  );
}
