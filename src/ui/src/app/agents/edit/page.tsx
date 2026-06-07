"use client";

import { Suspense } from "react";
import { useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { ArrowLeft, Puzzle } from "lucide-react";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { ModelSelect } from "@/components/model-select";
import { ScheduleEditor } from "@/components/schedule-editor";
import { getAgent, updateAgent, listModels, listMcpServers } from "@/lib/api";
import { DEFAULT_TIMEZONE } from "@/lib/schedule";
import type { McpServer } from "@/lib/types";

interface FormState {
  name: string;
  description: string;
  prompt: string;
  model: string;
  cron: string;
  timezone: string;
  mcp_server_ids: string[];
}

function AgentEdit() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const id = decodeURIComponent(searchParams.get("id") ?? "");

  const [form, setForm] = useState<FormState>({
    name: "",
    description: "",
    prompt: "",
    model: "",
    cron: "",
    timezone: DEFAULT_TIMEZONE,
    mcp_server_ids: [],
  });
  const [models, setModels] = useState<string[]>([]);
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    (async () => {
      try {
        const [ag, modelList, mcpServerList] = await Promise.all([
          getAgent(id),
          listModels(),
          listMcpServers(),
        ]);
        setForm({
          name: ag.name ?? "",
          description: ag.description ?? "",
          prompt: ag.prompt ?? "",
          model: ag.model ?? "",
          cron: ag.cron ?? "",
          timezone: ag.timezone ?? DEFAULT_TIMEZONE,
          mcp_server_ids: Array.isArray(ag.mcp_server_ids) ? ag.mcp_server_ids : [],
        });
        setModels(modelList);
        setMcpServers(mcpServerList);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setLoading(false);
      }
    })();
  }, [id]);

  const save = async () => {
    setSaving(true);
    setFormError(null);
    try {
      if (!form.name.trim()) throw new Error("Name is required");
      const cron = form.cron.trim();
      await updateAgent(id, {
        name: form.name,
        description: form.description,
        prompt: form.prompt,
        cron: cron || null,
        timezone: form.timezone.trim() || "UTC",
        mcp_server_ids: form.mcp_server_ids,
        ...(form.model ? { model: form.model } : {}),
      });
      router.push(`/agents/detail/?id=${encodeURIComponent(id)}`);
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const toggleMcpServer = (serverId: string) => {
    setForm((current) => ({
      ...current,
      mcp_server_ids: current.mcp_server_ids.includes(serverId)
        ? current.mcp_server_ids.filter((id) => id !== serverId)
        : [...current.mcp_server_ids, serverId],
    }));
  };

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar />
      <div className="flex-1 flex flex-col min-w-0">
        <header className="h-12 border-b border-border flex items-center justify-between px-4 shrink-0">
          <div className="flex items-center gap-2">
            <Button size="sm" variant="ghost"
              onClick={() => router.push(`/agents/detail/?id=${encodeURIComponent(id)}`)}
              className="gap-1.5 text-muted-foreground hover:text-foreground">
              <ArrowLeft className="size-3.5" />Agent
            </Button>
            <span className="text-muted-foreground">/</span>
            <span className="text-sm font-semibold">Edit</span>
          </div>
          <ThemeToggle />
        </header>
        <main className="flex-1 overflow-y-auto">
          <div className="max-w-2xl mx-auto px-4 py-8">
            {error && <Card className="border-destructive p-3 mb-6"><p className="text-sm text-destructive">{error}</p></Card>}
            {loading ? <div className="text-sm text-muted-foreground">Loading…</div> : (
              <div className="flex flex-col gap-6">
                <h1 className="text-lg font-semibold">Edit agent</h1>
                <div className="flex flex-col gap-4">
                  <div className="grid gap-1.5">
                    <Label htmlFor="ag-name">Name</Label>
                    <Input id="ag-name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="security-reviewer" />
                  </div>
                  <div className="grid gap-1.5">
                    <Label htmlFor="ag-desc">Description</Label>
                    <Input id="ag-desc" value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} placeholder="What this agent does" />
                  </div>
                  <div className="grid gap-1.5">
                    <Label>Model</Label>
                    <ModelSelect value={form.model} models={models} onValueChange={(v) => setForm({ ...form, model: v })} />
                  </div>
                  <div className="grid gap-1.5">
                    <Label htmlFor="ag-prompt">System prompt</Label>
                    <Textarea id="ag-prompt" value={form.prompt} onChange={(e) => setForm({ ...form, prompt: e.target.value })}
                      className="font-mono text-xs min-h-[320px] resize-y" placeholder="You are a meticulous security reviewer…" />
                  </div>
                  <ScheduleEditor
                    cron={form.cron}
                    timezone={form.timezone}
                    onChange={(next) => setForm({ ...form, ...next })}
                  />

                  <section className="grid gap-3 border-t border-border pt-5">
                    <div className="flex items-center justify-between gap-3">
                      <div>
                        <h2 className="text-sm font-semibold">MCP servers</h2>
                        <p className="text-xs text-muted-foreground">
                          Attach streamable HTTP MCP servers from the MCP Gateway.
                        </p>
                      </div>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => router.push("/integrations/")}
                      >
                        <Puzzle className="size-3.5" />
                        MCP Gateway
                      </Button>
                    </div>
                    <div className="grid gap-2">
                      {mcpServers.map((server) => {
                        const checked = form.mcp_server_ids.includes(server.id);
                        return (
                          <label
                            key={server.id}
                            className="grid cursor-pointer gap-2 rounded-lg border border-border bg-card p-3 sm:grid-cols-[auto_minmax(0,1fr)] sm:items-start"
                          >
                            <input
                              type="checkbox"
                              checked={checked}
                              onChange={() => toggleMcpServer(server.id)}
                              className="mt-0.5 size-4"
                            />
                            <span className="min-w-0">
                              <span className="flex items-center gap-2">
                                <span className="text-sm font-medium">{server.name}</span>
                                <span className="rounded border border-border px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
                                  {server.auth_type}
                                </span>
                              </span>
                              {server.description && (
                                <span className="mt-1 block text-xs text-muted-foreground">
                                  {server.description}
                                </span>
                              )}
                              <span className="mt-1 block break-all font-mono text-[11px] text-muted-foreground">
                                {server.url}
                              </span>
                            </span>
                          </label>
                        );
                      })}
                      {mcpServers.length === 0 && (
                        <div className="rounded-lg border border-dashed border-border py-8 text-center text-sm text-muted-foreground">
                          No MCP servers configured.
                        </div>
                      )}
                    </div>
                  </section>

                  {formError && (
                    <p className="text-sm text-destructive">{formError}</p>
                  )}
                </div>
                <div className="flex items-center gap-2 pt-2">
                  <Button onClick={save} disabled={saving}>{saving ? "Saving…" : "Save changes"}</Button>
                  <Button variant="outline" onClick={() => router.push(`/agents/detail/?id=${encodeURIComponent(id)}`)} disabled={saving}>Cancel</Button>
                </div>
              </div>
            )}
          </div>
        </main>
      </div>
    </div>
  );
}

export default function AgentEditPage() {
  return <Suspense><AgentEdit /></Suspense>;
}
