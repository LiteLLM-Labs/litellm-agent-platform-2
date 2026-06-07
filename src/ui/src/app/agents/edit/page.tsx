"use client";

import { Suspense } from "react";
import { useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { ArrowLeft } from "lucide-react";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { ModelSelect } from "@/components/model-select";
import { ScheduleEditor } from "@/components/schedule-editor";
import { getAgent, updateAgent, listAgentRuntimes, listModelOptions } from "@/lib/api";
import { modelIdsForRuntime } from "@/lib/runtime-models";
import { DEFAULT_TIMEZONE } from "@/lib/schedule";
import type { AgentRuntime, AgentRuntimeId, ModelOption } from "@/lib/types";

interface FormState {
  name: string;
  description: string;
  prompt: string;
  model: string;
  runtime: string;
  config: Record<string, unknown>;
  cron: string;
  timezone: string;
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
    runtime: "claude_managed_agents",
    config: {},
    cron: "",
    timezone: DEFAULT_TIMEZONE,
  });
  const [models, setModels] = useState<ModelOption[]>([]);
  const [runtimes, setRuntimes] = useState<AgentRuntime[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    (async () => {
      try {
        const [ag, modelList, runtimeList] = await Promise.all([getAgent(id), listModelOptions(), listAgentRuntimes()]);
        const config = (ag.config ?? {}) as { runtime?: string };
        const runtime = ag.runtime ?? (config.runtime as AgentRuntimeId | undefined) ?? "claude_managed_agents";
        setForm({
          name: ag.name ?? "",
          description: ag.description ?? "",
          prompt: ag.prompt ?? "",
          model: ag.model ?? "",
          runtime,
          config,
          cron: ag.cron ?? "",
          timezone: ag.timezone ?? DEFAULT_TIMEZONE,
        });
        setModels(modelList);
        setRuntimes(runtimeList);
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
        config: { ...form.config, runtime: form.runtime },
        cron: cron || null,
        timezone: form.timezone.trim() || "UTC",
        ...(form.model ? { model: form.model } : {}),
      });
      router.push(`/agents/detail/?id=${encodeURIComponent(id)}`);
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
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
                    <Label htmlFor="ag-runtime">Runtime</Label>
                    <select
                      id="ag-runtime"
                      value={form.runtime}
                      onChange={(e) => {
                        const runtime = e.target.value;
                        setForm({
                          ...form,
                          runtime,
                          model: modelIdsForRuntime(models, runtime, form.model)[0] ?? form.model,
                        });
                      }}
                      className="h-9 rounded-md border border-input bg-transparent px-3 text-sm outline-none focus-visible:border-ring dark:bg-input/30"
                    >
                      {runtimes.map((runtime) => (
                        <option key={runtime.id} value={runtime.id}>{runtime.name}</option>
                      ))}
                      {!runtimes.some((runtime) => runtime.id === form.runtime) && form.runtime && (
                        <option value={form.runtime}>{form.runtime}</option>
                      )}
                    </select>
                  </div>
                  <div className="grid gap-1.5">
                    <Label>Model</Label>
                    <ModelSelect
                      value={form.model}
                      models={modelIdsForRuntime(models, form.runtime, form.model)}
                      onValueChange={(v) => setForm({ ...form, model: v })}
                    />
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
