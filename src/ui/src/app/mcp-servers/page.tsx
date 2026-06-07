"use client";

import { useEffect, useState } from "react";
import {
  Server,
  Plus,
  Pencil,
  Trash2,
  Loader2,
  Search,
} from "lucide-react";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  listMcpServers,
  createMcpServer,
  updateMcpServer,
  deleteMcpServer,
  listMcpServerTools,
  discoverMcpToolsFromUrl,
} from "@/lib/api";
import type { McpToolDef } from "@/lib/api";
import type { McpServer } from "@/lib/types";

// ── Form state ────────────────────────────────────────────────────────────────

interface FormState {
  server_name: string;
  alias: string;
  description: string;
  url: string;
  transport: string;
  auth_type: string;
  is_byok: boolean;
  byok_description: string;
  byok_api_key_help_url: string;
  /** Array of selected tool names. Empty = allow all. */
  allowed_tools: string[];
  /** Fallback raw text when discovery hasn't been attempted or failed. */
  allowed_tools_text: string;
  available_on_public_internet: boolean;
}

const EMPTY_FORM: FormState = {
  server_name: "",
  alias: "",
  description: "",
  url: "",
  transport: "sse",
  auth_type: "none",
  is_byok: false,
  byok_description: "",
  byok_api_key_help_url: "",
  allowed_tools: [],
  allowed_tools_text: "",
  available_on_public_internet: false,
};

function serverToForm(s: McpServer): FormState {
  const tools = s.allowed_tools ?? [];
  return {
    server_name: s.server_name ?? "",
    alias: s.alias ?? "",
    description: s.description ?? "",
    url: s.url ?? "",
    transport: s.transport ?? "sse",
    auth_type: s.auth_type ?? "none",
    is_byok: s.is_byok ?? false,
    byok_description: (s.byok_description ?? []).join(", "),
    byok_api_key_help_url: s.byok_api_key_help_url ?? "",
    allowed_tools: tools,
    allowed_tools_text: tools.join(", "),
    available_on_public_internet: s.available_on_public_internet ?? false,
  };
}

function formToPayload(f: FormState, discoveredTools: McpToolDef[] | null): Partial<McpServer> {
  const byok = f.byok_description
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  // If we have discovery results, use the checkbox selection; otherwise parse the text field.
  const tools =
    discoveredTools !== null
      ? f.allowed_tools
      : f.allowed_tools_text
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean);
  return {
    server_name: f.server_name.trim() || undefined,
    alias: f.alias.trim() || undefined,
    description: f.description.trim() || undefined,
    url: f.url.trim(),
    transport: f.transport,
    auth_type: f.auth_type === "none" ? undefined : f.auth_type,
    is_byok: f.is_byok,
    byok_description: byok.length ? byok : undefined,
    byok_api_key_help_url: f.byok_api_key_help_url.trim() || undefined,
    allowed_tools: tools.length ? tools : undefined,
    available_on_public_internet: f.available_on_public_internet,
  };
}

// ── Page ──────────────────────────────────────────────────────────────────────

export default function McpServersPage() {
  const [servers, setServers] = useState<McpServer[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editorServer, setEditorServer] = useState<McpServer | null | "new">(null);

  const refresh = async () => {
    try {
      setServers(await listMcpServers());
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const onDelete = async (s: McpServer) => {
    if (!confirm(`Delete MCP server "${s.alias ?? s.server_name ?? s.server_id}"?`)) return;
    setServers((prev) => prev?.filter((x) => x.server_id !== s.server_id) ?? null);
    try {
      await deleteMcpServer(s.server_id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      await refresh();
    }
  };

  return (
    <div className="flex h-screen overflow-hidden bg-background">
      <Sidebar />
      <div className="flex flex-1 flex-col overflow-hidden">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
          <div className="flex items-center gap-2">
            <Server className="size-4 text-muted-foreground" />
            <h1 className="text-sm font-semibold">MCP Servers</h1>
          </div>
          <div className="flex items-center gap-2">
            <Button size="sm" onClick={() => setEditorServer("new")}>
              <Plus className="size-4" />
              Add Server
            </Button>
            <ThemeToggle />
          </div>
        </header>

        <main className="flex-1 overflow-y-auto p-6">
          {error && (
            <div className="mb-4 rounded-lg border border-destructive/40 bg-destructive/10 px-4 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          {servers === null && !error && (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" />
              Loading…
            </div>
          )}

          {servers !== null && servers.length === 0 && (
            <div className="flex flex-col items-center justify-center gap-3 py-16 text-center">
              <Server className="size-10 text-muted-foreground/40" />
              <p className="text-sm text-muted-foreground">No MCP servers registered yet.</p>
              <Button size="sm" onClick={() => setEditorServer("new")}>
                <Plus className="size-4" />
                Add your first server
              </Button>
            </div>
          )}

          {servers !== null && servers.length > 0 && (
            <div className="max-w-5xl">
              <div className="rounded-lg border border-border overflow-hidden">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-border bg-muted/40">
                      <th className="px-4 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">
                        Name
                      </th>
                      <th className="px-4 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">
                        URL
                      </th>
                      <th className="px-4 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">
                        Transport
                      </th>
                      <th className="px-4 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">
                        Flags
                      </th>
                      <th className="px-4 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">
                        Status
                      </th>
                      <th className="px-4 py-2.5 text-right text-xs font-medium text-muted-foreground uppercase tracking-wide">
                        Actions
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border">
                    {servers.map((s) => (
                      <ServerRow
                        key={s.server_id}
                        server={s}
                        onEdit={() => setEditorServer(s)}
                        onDelete={() => onDelete(s)}
                      />
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
        </main>
      </div>

      <McpServerEditor
        serverOrNew={editorServer}
        onClose={() => setEditorServer(null)}
        onSaved={() => {
          setEditorServer(null);
          refresh();
        }}
      />
    </div>
  );
}

// ── Table row ─────────────────────────────────────────────────────────────────

function ServerRow({
  server,
  onEdit,
  onDelete,
}: {
  server: McpServer;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const displayName = server.alias ?? server.server_name ?? server.server_id;
  const status = server.status ?? "unknown";

  return (
    <tr className="group bg-card hover:bg-muted/30 transition-colors">
      <td className="px-4 py-3">
        <div className="font-medium text-sm">{displayName}</div>
        {server.description && (
          <div className="text-xs text-muted-foreground mt-0.5 line-clamp-1">
            {server.description}
          </div>
        )}
      </td>
      <td className="px-4 py-3">
        <span className="font-mono text-xs text-muted-foreground truncate max-w-xs block">
          {server.url ?? "—"}
        </span>
      </td>
      <td className="px-4 py-3">
        <Badge variant="outline" className="text-[10px] uppercase font-mono">
          {server.transport}
        </Badge>
      </td>
      <td className="px-4 py-3">
        <div className="flex flex-wrap gap-1">
          {server.is_byok && (
            <Badge className="text-[10px] bg-amber-500/10 text-amber-700 dark:text-amber-400 border-amber-500/30">
              BYOK
            </Badge>
          )}
          {server.available_on_public_internet && (
            <Badge className="text-[10px] bg-blue-500/10 text-blue-700 dark:text-blue-400 border-blue-500/30">
              Public
            </Badge>
          )}
        </div>
      </td>
      <td className="px-4 py-3">
        <Badge
          variant={status === "active" ? "secondary" : "outline"}
          className={`text-[10px] ${
            status === "active"
              ? "bg-green-500/10 text-green-700 dark:text-green-400 border-green-500/30"
              : "text-muted-foreground"
          }`}
        >
          {status}
        </Badge>
      </td>
      <td className="px-4 py-3">
        <div className="flex items-center justify-end gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
          <Button
            size="sm"
            variant="ghost"
            onClick={onEdit}
            aria-label="Edit server"
          >
            <Pencil className="size-3.5" />
          </Button>
          <Button
            size="sm"
            variant="ghost"
            className="text-destructive hover:text-destructive"
            onClick={onDelete}
            aria-label="Delete server"
          >
            <Trash2 className="size-3.5" />
          </Button>
        </div>
      </td>
    </tr>
  );
}

// ── Add/Edit modal ────────────────────────────────────────────────────────────

function McpServerEditor({
  serverOrNew,
  onClose,
  onSaved,
}: {
  serverOrNew: McpServer | "new" | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const isEdit = serverOrNew !== null && serverOrNew !== "new";
  const open = serverOrNew !== null;

  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Tool discovery state
  const [discoveredTools, setDiscoveredTools] = useState<McpToolDef[] | null>(null);
  const [discovering, setDiscovering] = useState(false);
  const [discoverError, setDiscoverError] = useState<string | null>(null);

  useEffect(() => {
    if (serverOrNew === "new") {
      setForm(EMPTY_FORM);
    } else if (serverOrNew !== null) {
      setForm(serverToForm(serverOrNew));
    }
    setError(null);
    setDiscoveredTools(null);
    setDiscoverError(null);
  }, [serverOrNew]);

  const patch = <K extends keyof FormState>(key: K, value: FormState[K]) =>
    setForm((f) => ({ ...f, [key]: value }));

  const onDiscoverTools = async () => {
    setDiscoverError(null);
    setDiscovering(true);
    try {
      let tools: McpToolDef[];
      if (isEdit) {
        tools = await listMcpServerTools((serverOrNew as McpServer).server_id);
      } else {
        const url = form.url.trim();
        if (!url) {
          setDiscoverError("Enter a URL before discovering tools.");
          return;
        }
        tools = await discoverMcpToolsFromUrl(url);
      }
      setDiscoveredTools(tools);
      // Pre-select tools that were already in allowed_tools
      const alreadySelected = new Set(form.allowed_tools);
      if (alreadySelected.size > 0) {
        // Keep existing selection intact — it was loaded from the saved server.
      } else {
        // No prior selection: select all by default so the admin can uncheck unwanted ones.
        setForm((f) => ({ ...f, allowed_tools: tools.map((t) => t.name) }));
      }
    } catch (e) {
      setDiscoverError(e instanceof Error ? e.message : String(e));
      setDiscoveredTools(null);
    } finally {
      setDiscovering(false);
    }
  };

  const toggleTool = (name: string, checked: boolean) => {
    setForm((f) => ({
      ...f,
      allowed_tools: checked
        ? [...f.allowed_tools, name]
        : f.allowed_tools.filter((t) => t !== name),
    }));
  };

  const onSave = async () => {
    const url = form.url.trim();
    if (!url) {
      setError("URL is required.");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const payload = formToPayload(form, discoveredTools);
      if (isEdit) {
        await updateMcpServer((serverOrNew as McpServer).server_id, payload);
      } else {
        await createMcpServer(payload);
      }
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onClose(); }}>
      <DialogContent className="w-[92vw] sm:max-w-xl max-h-[88vh] flex flex-col gap-0 p-0">
        <DialogHeader className="px-6 pt-6 pb-4 border-b border-border shrink-0">
          <DialogTitle>{isEdit ? "Edit MCP Server" : "Add MCP Server"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Update the registration for this MCP server."
              : "Register a new MCP server that agents can connect to."}
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* server_name */}
          <div className="space-y-1.5">
            <Label htmlFor="mcp-server-name">Server name</Label>
            <Input
              id="mcp-server-name"
              value={form.server_name}
              onChange={(e) => patch("server_name", e.target.value)}
              placeholder="my-mcp-server"
            />
          </div>

          {/* alias */}
          <div className="space-y-1.5">
            <Label htmlFor="mcp-alias">Alias</Label>
            <Input
              id="mcp-alias"
              value={form.alias}
              onChange={(e) => patch("alias", e.target.value)}
              placeholder="Human-readable shortname"
            />
          </div>

          {/* description */}
          <div className="space-y-1.5">
            <Label htmlFor="mcp-description">Description</Label>
            <textarea
              id="mcp-description"
              value={form.description}
              onChange={(e) => patch("description", e.target.value)}
              placeholder="What this MCP server provides…"
              rows={2}
              className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring resize-none"
            />
          </div>

          {/* url (required) */}
          <div className="space-y-1.5">
            <Label htmlFor="mcp-url">
              URL <span className="text-destructive">*</span>
            </Label>
            <Input
              id="mcp-url"
              value={form.url}
              onChange={(e) => patch("url", e.target.value)}
              placeholder="https://my-mcp-server.example.com/sse"
              required
            />
          </div>

          {/* transport */}
          <div className="space-y-1.5">
            <Label htmlFor="mcp-transport">Transport</Label>
            <select
              id="mcp-transport"
              value={form.transport}
              onChange={(e) => patch("transport", e.target.value)}
              className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <option value="sse">SSE</option>
              <option value="http">HTTP</option>
              <option value="stdio">stdio</option>
            </select>
          </div>

          {/* auth_type */}
          <div className="space-y-1.5">
            <Label htmlFor="mcp-auth-type">Auth type</Label>
            <select
              id="mcp-auth-type"
              value={form.auth_type}
              onChange={(e) => patch("auth_type", e.target.value)}
              className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <option value="none">None</option>
              <option value="bearer_token">Bearer token</option>
              <option value="api_key">API key</option>
              <option value="basic">Basic auth</option>
            </select>
          </div>

          {/* is_byok */}
          <div className="space-y-2">
            <label className="flex items-center gap-2 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={form.is_byok}
                onChange={(e) => patch("is_byok", e.target.checked)}
                className="rounded"
              />
              <span className="text-sm font-medium">
                Users provide their own key (BYOK)
              </span>
            </label>

            {form.is_byok && (
              <div className="ml-6 space-y-3 rounded-md border border-border p-3">
                <div className="space-y-1.5">
                  <Label htmlFor="mcp-byok-desc" className="text-xs">
                    Key names <span className="text-muted-foreground">(comma-separated)</span>
                  </Label>
                  <Input
                    id="mcp-byok-desc"
                    value={form.byok_description}
                    onChange={(e) => patch("byok_description", e.target.value)}
                    placeholder="MY_API_KEY, MY_SECRET"
                    className="font-mono text-xs"
                  />
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="mcp-byok-url" className="text-xs">
                    Help URL <span className="text-muted-foreground">(optional)</span>
                  </Label>
                  <Input
                    id="mcp-byok-url"
                    value={form.byok_api_key_help_url}
                    onChange={(e) => patch("byok_api_key_help_url", e.target.value)}
                    placeholder="https://docs.example.com/api-keys"
                    className="text-xs"
                  />
                </div>
              </div>
            )}
          </div>

          {/* allowed_tools */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label>
                Allowed tools{" "}
                <span className="text-xs font-normal text-muted-foreground">
                  (leave empty to allow all)
                </span>
              </Label>
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={onDiscoverTools}
                disabled={discovering}
                className="h-7 gap-1.5 text-xs"
              >
                {discovering ? (
                  <Loader2 className="size-3 animate-spin" />
                ) : (
                  <Search className="size-3" />
                )}
                {discovering ? "Discovering…" : "Discover tools"}
              </Button>
            </div>

            {discoverError && (
              <p className="text-xs text-destructive">{discoverError}</p>
            )}

            {discoveredTools !== null ? (
              discoveredTools.length === 0 ? (
                <p className="text-xs text-muted-foreground italic">
                  No tools returned by the server.
                </p>
              ) : (
                <div className="rounded-md border border-border divide-y divide-border max-h-48 overflow-y-auto">
                  {discoveredTools.map((tool) => {
                    const checked = form.allowed_tools.includes(tool.name);
                    return (
                      <label
                        key={tool.name}
                        className="flex items-start gap-2.5 px-3 py-2 cursor-pointer hover:bg-muted/30 transition-colors"
                      >
                        <input
                          type="checkbox"
                          checked={checked}
                          onChange={(e) => toggleTool(tool.name, e.target.checked)}
                          className="mt-0.5 rounded shrink-0"
                        />
                        <div className="min-w-0">
                          <span className="block text-xs font-mono font-medium leading-tight">
                            {tool.name}
                          </span>
                          {tool.description && (
                            <span className="block text-[11px] text-muted-foreground leading-tight mt-0.5 line-clamp-2">
                              {tool.description}
                            </span>
                          )}
                        </div>
                      </label>
                    );
                  })}
                </div>
              )
            ) : (
              <Input
                id="mcp-allowed-tools"
                value={form.allowed_tools_text}
                onChange={(e) => patch("allowed_tools_text", e.target.value)}
                placeholder="read_file, write_file"
                className="font-mono text-xs"
              />
            )}
          </div>

          {/* available_on_public_internet */}
          <div>
            <label className="flex items-center gap-2 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={form.available_on_public_internet}
                onChange={(e) =>
                  patch("available_on_public_internet", e.target.checked)
                }
                className="rounded"
              />
              <span className="text-sm font-medium">
                Show in public hub
              </span>
            </label>
            <p className="ml-6 mt-0.5 text-xs text-muted-foreground">
              Makes this server discoverable in the public integration hub.
            </p>
          </div>

          {error && (
            <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-2 border-t border-border px-6 py-4 shrink-0">
          <Button variant="outline" onClick={onClose} disabled={saving}>
            Cancel
          </Button>
          <Button onClick={onSave} disabled={saving}>
            {saving ? (
              <>
                <Loader2 className="size-4 animate-spin" />
                Saving…
              </>
            ) : isEdit ? (
              "Save"
            ) : (
              "Add server"
            )}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
