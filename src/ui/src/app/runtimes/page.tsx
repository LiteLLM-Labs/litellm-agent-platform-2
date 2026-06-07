"use client";

import { useCallback, useEffect, useState } from "react";
import { Check, KeyRound, ServerCog, X } from "lucide-react";

import { BrandIcon } from "@/components/brand-icons";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  deleteAgentRuntimeCredential,
  listAgentRuntimes,
  saveAgentRuntimeCredential,
} from "@/lib/api";
import type { AgentRuntime, AgentRuntimeId } from "@/lib/types";

export default function RuntimesPage() {
  const [runtimes, setRuntimes] = useState<AgentRuntime[]>([]);
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [bases, setBases] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const next = await listAgentRuntimes();
    setRuntimes(next);
    setBases((current) => {
      const updated = { ...current };
      for (const runtime of next) {
        updated[runtime.id] = runtime.api_base ?? runtime.default_api_base;
      }
      return updated;
    });
  }, []);

  useEffect(() => {
    refresh()
      .catch((err) => setError(err instanceof Error ? err.message : "Failed to load runtimes"))
      .finally(() => setLoading(false));
  }, [refresh]);

  const saveRuntime = async (runtime: AgentRuntimeId) => {
    const apiKey = keys[runtime]?.trim();
    if (!apiKey) return;
    setSaving(true);
    setError(null);
    try {
      const next = await saveAgentRuntimeCredential({
        runtime,
        apiKey,
        apiBase: bases[runtime],
      });
      setRuntimes(next);
      setKeys((current) => ({ ...current, [runtime]: "" }));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save runtime");
    } finally {
      setSaving(false);
    }
  };

  const disconnectRuntime = async (runtime: AgentRuntimeId) => {
    setSaving(true);
    setError(null);
    try {
      await deleteAgentRuntimeCredential(runtime);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to disconnect runtime");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
          <div className="flex items-center gap-2">
            <ServerCog className="size-4 text-muted-foreground" />
            <h1 className="text-sm font-semibold">Agent Runtimes</h1>
          </div>
          <ThemeToggle />
        </header>
        <main id="main-content" className="flex-1 overflow-y-auto">
          <div className="mx-auto grid w-[calc(100vw-4rem)] max-w-5xl gap-5 px-4 py-6 sm:w-full">
            <div className="min-w-0">
              <h2 className="text-lg font-semibold tracking-tight">Agent Runtime Credentials</h2>
              <p className="text-sm text-muted-foreground">
                Connect SDK agent runtimes before starting runtime sessions.
              </p>
              {loading && <p className="mt-2 text-xs text-muted-foreground">Loading runtimes…</p>}
              {error && <p className="mt-2 text-xs text-destructive">{error}</p>}
            </div>

            <div className="grid gap-3 md:grid-cols-2">
              {runtimes.map((runtime) => (
                <Card key={runtime.id} className="grid min-w-0 gap-4 p-4">
                  <div className="flex items-start justify-between gap-3">
                    <div className="flex min-w-0 items-center gap-3">
                      <RuntimeLogo id={runtime.id} />
                      <div className="min-w-0">
                        <div className="font-medium">{runtime.name}</div>
                        <p className="truncate font-mono text-xs text-muted-foreground">
                          {runtime.masked_api_key ?? "No API key"}
                        </p>
                      </div>
                    </div>
                    <Badge variant={runtime.connected ? "secondary" : "outline"} className="text-[10px]">
                      {runtime.connected ? "Connected" : "Missing"}
                    </Badge>
                  </div>
                  <div className="grid gap-3">
                    <div className="grid gap-1.5">
                      <Label htmlFor={`runtime-key-${runtime.id}`}>API key</Label>
                      <div className="relative">
                        <KeyRound className="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                        <Input
                          id={`runtime-key-${runtime.id}`}
                          type="password"
                          value={keys[runtime.id] ?? ""}
                          onChange={(event) =>
                            setKeys((current) => ({
                              ...current,
                              [runtime.id]: event.target.value,
                            }))
                          }
                          placeholder="Runtime API key"
                          className="pl-8 font-mono text-xs"
                        />
                      </div>
                    </div>
                    <div className="grid gap-1.5">
                      <Label htmlFor={`runtime-base-${runtime.id}`}>API base</Label>
                      <Input
                        id={`runtime-base-${runtime.id}`}
                        value={bases[runtime.id] ?? runtime.default_api_base}
                        onChange={(event) =>
                          setBases((current) => ({
                            ...current,
                            [runtime.id]: event.target.value,
                          }))
                        }
                        className="font-mono text-xs"
                      />
                    </div>
                  </div>
                  <div className="flex justify-end gap-2">
                    {runtime.connected && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => disconnectRuntime(runtime.id)}
                        disabled={saving}
                      >
                        <X className="size-3.5" />
                        Disconnect
                      </Button>
                    )}
                    <Button
                      size="sm"
                      onClick={() => saveRuntime(runtime.id)}
                      disabled={saving || !(keys[runtime.id] ?? "").trim()}
                    >
                      <Check className="size-3.5" />
                      Save
                    </Button>
                  </div>
                </Card>
              ))}
            </div>
          </div>
        </main>
      </div>
    </div>
  );
}

function RuntimeLogo({ id }: { id: AgentRuntimeId }) {
  return (
    <span className="flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground shadow-sm">
      <BrandIcon id={runtimeIconId(id)} className="size-5" />
    </span>
  );
}

function runtimeIconId(id: AgentRuntimeId) {
  return id === "claude_managed_agents" ? "claude" : id;
}
