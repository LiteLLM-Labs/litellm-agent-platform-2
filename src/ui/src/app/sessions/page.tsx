"use client";

import { useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { Bot, Play, ServerCog } from "lucide-react";
import { BrandIcon } from "@/components/brand-icons";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { createSession, listAgentRuntimes, listAgents } from "@/lib/api";
import type { Agent, AgentRuntime, AgentRuntimeId } from "@/lib/types";

function runtimeIconId(id: AgentRuntimeId) {
  return id === "claude_agents" ? "claude" : id;
}

export default function SessionsPage() {
  const router = useRouter();
  const [runtime, setRuntime] = useState<AgentRuntimeId>("cursor");
  const [agents, setAgents] = useState<Agent[]>([]);
  const [agentId, setAgentId] = useState("");
  const [runtimes, setRuntimes] = useState<AgentRuntime[]>([]);
  const [repository, setRepository] = useState("");
  const [ref, setRef] = useState("main");
  const [prompt, setPrompt] = useState("Start a runtime session for this agent.");
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void Promise.all([listAgents(), listAgentRuntimes()])
      .then(([nextAgents, nextRuntimes]) => {
        setAgents(nextAgents);
        setRuntimes(nextRuntimes);
        if (nextAgents[0]) setAgentId(nextAgents[0].id);
      })
      .catch((err) => setError(err instanceof Error ? err.message : "Failed to load"));
  }, []);

  const selectedRuntime = useMemo(
    () => runtimes.find((item) => item.id === runtime),
    [runtime, runtimes],
  );
  const selectedAgent = useMemo(
    () => agents.find((agent) => agent.id === agentId),
    [agentId, agents],
  );

  const startSession = async () => {
    if (!agentId) return;
    setStarting(true);
    setError(null);
    try {
      const environment =
        runtime === "cursor"
          ? {
              repository,
              ref,
              target_branch: "agent/{agent_id}/{session_id}",
              auto_create_pr: false,
            }
          : {};
      const session = await createSession("Runtime PoC", agentId, {
        runtime,
        prompt,
        environment,
      });
      router.push(`/chat/?id=${encodeURIComponent(session.id)}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to start session");
    } finally {
      setStarting(false);
    }
  };

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar />
      <div className="flex-1 flex flex-col min-w-0">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
          <div className="flex min-w-0 items-center gap-2">
            <ServerCog className="size-4 text-muted-foreground" />
            <span className="truncate text-sm font-medium">Start Session</span>
          </div>
          <ThemeToggle />
        </header>
        <main className="flex-1 overflow-y-auto p-4 sm:p-6">
          <div className="mx-auto grid max-w-5xl gap-4">
            <div>
              <div>
                <h1 className="text-xl font-semibold tracking-tight">Start Session</h1>
                <p className="text-sm text-muted-foreground">
                  Select an agent and runtime. The backend provisions the downstream session.
                </p>
              </div>
            </div>

            <div className="grid gap-4">
              <Card className="grid gap-4 p-4">
                <div className="grid gap-1.5">
                  <Label>Agent</Label>
                  <Select value={agentId} onValueChange={(value) => value && setAgentId(value)}>
                    <SelectTrigger>
                      <SelectValue placeholder="Select agent">
                        {selectedAgent?.name}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      {agents.map((agent) => (
                        <SelectItem key={agent.id} value={agent.id}>
                          <span className="flex items-center gap-2">
                            <Bot className="size-3.5" />
                            {agent.name}
                          </span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>

                <div className="grid gap-1.5">
                  <Label>Runtime</Label>
                  <Select value={runtime} onValueChange={(v) => v && setRuntime(v as AgentRuntimeId)}>
                    <SelectTrigger>
                      <SelectValue>
                        <span className="flex items-center gap-2">
                          <span className="flex size-5 items-center justify-center rounded border border-border bg-background">
                            <BrandIcon id={runtimeIconId(runtime)} className="size-3.5" />
                          </span>
                          {selectedRuntime?.name ?? runtime}
                        </span>
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      {runtimes.map((item) => (
                        <SelectItem key={item.id} value={item.id}>
                          <span className="flex items-center gap-2">
                            <span className="flex size-5 items-center justify-center rounded border border-border bg-background">
                              <BrandIcon id={runtimeIconId(item.id)} className="size-3.5" />
                            </span>
                            {item.name}
                            <span className="text-muted-foreground">
                              {item.connected ? "" : "missing key"}
                            </span>
                          </span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>

                {runtime === "cursor" && (
                  <div className="grid gap-3 sm:grid-cols-2">
                    <div className="grid gap-1.5 sm:col-span-2">
                      <Label htmlFor="repo">Repository</Label>
                      <Input
                        id="repo"
                        value={repository}
                        onChange={(event) => setRepository(event.target.value)}
                        placeholder="https://github.com/org/repo"
                      />
                    </div>
                    <div className="grid gap-1.5">
                      <Label htmlFor="ref">Ref</Label>
                      <Input id="ref" value={ref} onChange={(event) => setRef(event.target.value)} />
                    </div>
                  </div>
                )}

                <div className="grid gap-1.5">
                  <Label htmlFor="prompt">Prompt</Label>
                  <Textarea
                    id="prompt"
                    value={prompt}
                    onChange={(event) => setPrompt(event.target.value)}
                    className="min-h-24"
                  />
                </div>

                {error && <p className="rounded-md border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">{error}</p>}
                <div className="flex justify-end">
                  <Button
                    onClick={startSession}
                    disabled={
                      starting ||
                      !agentId ||
                      !selectedRuntime?.connected ||
                      (runtime === "cursor" && !repository.trim())
                    }
                  >
                    <Play className="size-4" />
                    {starting ? "Starting..." : "Start session"}
                  </Button>
                </div>
              </Card>
            </div>
          </div>
        </main>
      </div>
    </div>
  );
}
