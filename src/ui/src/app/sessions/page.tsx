"use client";

import { Suspense, useEffect, useMemo, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { ArrowUp, Bot, Mic, Paperclip } from "lucide-react";
import { BrandIcon } from "@/components/brand-icons";
import { Sidebar } from "@/components/sidebar";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import {
  createAgent,
  createSession,
  listAgentRuntimes,
  listAgents,
  listSessions,
} from "@/lib/api";
import type { Agent, AgentRuntime, AgentRuntimeId } from "@/lib/types";

const NEW_AGENT_VALUE = "__new_agent__";
const CLAUDE_RUNTIME: AgentRuntimeId = "claude_managed_agents";

function runtimeIconId(id: string) {
  return id === "claude_managed_agents" || id === "claude_agents" ? "claude" : id;
}

function runtimeLabel(runtime: AgentRuntime | string): string {
  if (typeof runtime !== "string") return runtime.name;
  if (runtime === "claude_managed_agents") return "Claude Agents";
  if (runtime === "cursor") return "Cursor";
  if (runtime === "opencode") return "OpenCode";
  if (runtime === "claude-code" || runtime === "cc") return "Claude Code";
  return runtime;
}

function runtimeSubtitle(runtime: AgentRuntime): string {
  if (!runtime.connected) return "missing key";
  if (runtime.id === "claude_managed_agents") return "Anthropic sessions and tools";
  if (runtime.id === "cursor") return "Background repo agents";
  if (runtime.id === "opencode") return "OpenCode server sessions";
  return "Managed runtime sessions";
}

function modelForRuntime(runtime: AgentRuntimeId): string {
  if (runtime === "claude_managed_agents") return "claude-sonnet-4-6";
  if (runtime === "opencode") return "opencode/default";
  return "claude-4-sonnet";
}

function runtimeRoutePrefix(runtime: AgentRuntimeId | ""): string {
  if (runtime === "claude_managed_agents") return "anthropic/*";
  if (runtime === "cursor") return "cursor/*";
  if (runtime === "opencode") return "opencode/*";
  return "runtime/*";
}

function promptTitle(prompt: string): string {
  const compact = prompt.replace(/\s+/g, " ").trim();
  if (!compact) return "New agent session";
  return compact.length > 46 ? `${compact.slice(0, 46).trimEnd()}...` : compact;
}

function isDbBackedAgent(agent: Agent): boolean {
  return agent.id.startsWith("agent_");
}

function cursorEnvironment(repository: string, ref: string): Record<string, unknown> {
  return {
    repository: repository.trim(),
    ref: ref.trim() || "main",
    target_branch: "agent/{agent_id}/{session_id}",
    auto_create_pr: false,
  };
}

function SessionsStart() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [selectedAgentId, setSelectedAgentId] = useState(searchParams.get("agent") ?? "");
  const [prompt, setPrompt] = useState("");
  const [runtime, setRuntime] = useState<AgentRuntimeId | "">("");
  const [runtimes, setRuntimes] = useState<AgentRuntime[]>([]);
  const [savedAgents, setSavedAgents] = useState<Agent[]>([]);
  const [repository, setRepository] = useState("");
  const [ref, setRef] = useState("main");
  const [sessionCount, setSessionCount] = useState<number | null>(null);
  const [agentCount, setAgentCount] = useState<number | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([listAgentRuntimes(), listSessions(), listAgents()])
      .then(([nextRuntimes, nextSessions, nextAgents]) => {
        setRuntimes(nextRuntimes);
        setRuntime((current) => {
          if (current && nextRuntimes.some((item) => item.id === current)) return current;
          return nextRuntimes.find((item) => item.id === CLAUDE_RUNTIME)?.id ?? nextRuntimes[0]?.id ?? "";
        });
        setSessionCount(nextSessions.length);
        setAgentCount(nextAgents.length);
        setSavedAgents(nextAgents);
      })
      .catch((err) => setError(err instanceof Error ? err.message : "Failed to load runtimes"));
  }, []);

  const selectedRuntime = useMemo(
    () => runtimes.find((item) => item.id === runtime),
    [runtime, runtimes],
  );
  const selectedAgent = useMemo(
    () => savedAgents.find((agent) => agent.id === selectedAgentId) ?? null,
    [savedAgents, selectedAgentId],
  );
  const selectedAgentMissing = selectedAgentId !== "" && selectedAgent === null;
  const selectedAgentIsConfigured = Boolean(selectedAgent && !isDbBackedAgent(selectedAgent));
  const needsRuntime = !selectedAgentIsConfigured;
  const needsPrompt = !selectedAgent;
  const canStart =
    (!needsPrompt || prompt.trim().length > 0) &&
    !starting &&
    !selectedAgentMissing &&
    (!needsRuntime ||
      (runtime !== "" &&
        Boolean(selectedRuntime?.connected) &&
        (runtime !== "cursor" || repository.trim().length > 0)));

  const startSession = async () => {
    const trimmed = prompt.trim();
    const runtimeId = runtime;
    if (selectedAgentMissing) {
      setError("Selected agent is no longer available.");
      return;
    }
    if (
      starting ||
      !canStart ||
      (needsRuntime && !runtimeId) ||
      (needsPrompt && !trimmed)
    ) {
      return;
    }
    setStarting(true);
    setError(null);
    try {
      const title = selectedAgent ? `${selectedAgent.name} session` : promptTitle(trimmed);
      const session =
        selectedAgent && !isDbBackedAgent(selectedAgent)
          ? await createSession(title, selectedAgent.id)
          : await (async () => {
              const runtimeForSession = runtimeId as AgentRuntimeId;
              const agent =
                selectedAgent ??
                (await createAgent({
                  name: title,
                  owner_id: "default",
                  description: `Started from ${runtimeLabel(selectedRuntime ?? runtimeForSession)} landing prompt.`,
                  model: modelForRuntime(runtimeForSession),
                  runtime: runtimeForSession,
                  harness: "claude-code",
                  system: "You are a helpful managed agent. Use available tools when they help complete the user's request.",
                  tools: [{ type: "agent_toolset_20260401" }],
                  mcp_servers: [],
                  skills: [],
                }));
              const environment =
                runtimeForSession === "cursor" ? cursorEnvironment(repository, ref) : {};
              return createSession(title, agent.id, {
                runtime: runtimeForSession,
                environment,
              });
            })();
      const params = new URLSearchParams({
        id: session.id,
      });
      if (trimmed) {
        params.set("prompt", trimmed);
        params.set("autostart", "1");
      }
      router.push(`/chat/?${params.toString()}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to start session");
    } finally {
      setStarting(false);
    }
  };

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar />
      <main className="relative flex min-w-0 flex-1 overflow-hidden bg-[#fbfbfa] text-[#20201f]">
        <div
          aria-hidden
          className="absolute inset-0 opacity-80"
          style={{
            backgroundImage:
              "radial-gradient(circle at center, rgba(59, 130, 246, 0.18) 1px, transparent 1.4px)",
            backgroundSize: "10px 10px",
          }}
        />
        <div
          aria-hidden
          className="absolute inset-x-0 bottom-0 h-[48%] opacity-70"
          style={{
            background:
              "radial-gradient(ellipse at 52% 15%, rgba(59,130,246,0.26), rgba(59,130,246,0.08) 42%, transparent 72%)",
          }}
        />

        <section className="relative z-10 flex min-h-full w-full flex-col items-center justify-center px-6 py-12">
          <div className="w-full max-w-2xl overflow-hidden rounded-lg border border-black/10 bg-white/92 shadow-[0_18px_70px_rgba(15,23,42,0.12)] backdrop-blur">
            <Textarea
              value={prompt}
              onChange={(event) => setPrompt(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  void startSession();
                }
              }}
              placeholder={selectedAgent ? "Optional first message" : "Ask or build anything"}
              className="min-h-24 resize-none border-0 bg-transparent px-4 py-4 text-[15px] text-[#20201f] shadow-none outline-none placeholder:text-[#77736d] focus-visible:ring-0"
            />
            <div className="flex flex-wrap items-center gap-2 border-t border-black/10 bg-[#faf9f7] px-3 py-3">
              <Select
                value={selectedAgentId || NEW_AGENT_VALUE}
                onValueChange={(value) => {
                  const next = value ?? "";
                  setSelectedAgentId(next === NEW_AGENT_VALUE ? "" : next);
                }}
              >
                <SelectTrigger className="h-10 w-auto min-w-[230px] max-w-[320px] rounded-full border border-black/10 bg-white px-3 text-left text-[#20201f] shadow-sm transition-colors hover:bg-[#fbfaf8] focus:ring-1 focus:ring-black/15">
                  <span className="flex min-w-0 items-center gap-2">
                    <span className="shrink-0 text-[10px] font-semibold uppercase tracking-wide text-[#77736d]">
                      Agent
                    </span>
                    <span className="truncate text-sm font-medium">
                      {selectedAgent?.name ?? "New managed agent"}
                    </span>
                  </span>
                </SelectTrigger>
                <SelectContent className="w-[340px]">
                  <SelectItem value={NEW_AGENT_VALUE} className="py-3">
                    <span className="flex min-w-0 items-center gap-3">
                      <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-border bg-background">
                        <Bot className="size-4" />
                      </span>
                      <span className="min-w-0">
                        <span className="block truncate text-sm font-medium">New managed agent</span>
                        <span className="block truncate text-xs text-muted-foreground">
                          Create one from this prompt
                        </span>
                      </span>
                    </span>
                  </SelectItem>
                  {savedAgents.length > 0 && (
                    <>
                      <div className="px-2 py-1.5 text-[10px] uppercase tracking-wider text-muted-foreground">
                        Saved agents
                      </div>
                      {savedAgents.map((agent) => (
                        <SelectItem key={agent.id} value={agent.id} className="py-3">
                          <span className="block truncate text-sm font-medium">{agent.name}</span>
                        </SelectItem>
                      ))}
                    </>
                  )}
                </SelectContent>
              </Select>

              <Select value={runtime} onValueChange={(value) => setRuntime((value ?? "") as AgentRuntimeId | "")}>
                <SelectTrigger className="h-10 w-auto min-w-[260px] max-w-[340px] rounded-full border border-black/10 bg-white px-3 text-left text-[#20201f] shadow-sm transition-colors hover:bg-[#fbfaf8] focus:ring-1 focus:ring-black/15">
                  <span className="flex min-w-0 items-center gap-2">
                    <span className="shrink-0 text-[10px] font-semibold uppercase tracking-wide text-[#77736d]">
                      Runtime
                    </span>
                    <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-[#f3f1ee]">
                      <BrandIcon id={runtimeIconId(runtime)} className="size-4" />
                    </span>
                    <span className="truncate text-sm font-medium">
                      {selectedRuntime?.name ?? (runtime ? runtimeLabel(runtime) : "Select runtime")}
                    </span>
                  </span>
                </SelectTrigger>
                <SelectContent className="w-[340px]">
                  {runtimes.map((item) => (
                    <SelectItem key={item.id} value={item.id} disabled={!item.connected} className="py-3">
                      <span className="flex min-w-0 items-center gap-3">
                        <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-border bg-background">
                          <BrandIcon id={runtimeIconId(item.id)} className="size-4" />
                        </span>
                        <span className="min-w-0">
                          <span className="block truncate text-sm font-medium">{item.name}</span>
                          <span className="block truncate text-xs text-muted-foreground">
                            {runtimeSubtitle(item)}
                          </span>
                        </span>
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="hidden rounded-full border border-black/10 bg-white px-3 py-1.5 font-mono text-xs text-[#77736d] sm:inline">
                {selectedAgentIsConfigured ? "agent/*" : runtimeRoutePrefix(runtime)}
              </span>
              <Button variant="ghost" size="icon-sm" disabled className="ml-auto text-[#5d5a55]">
                <Mic className="size-4" />
              </Button>
              <Button variant="ghost" size="icon-sm" disabled className="text-[#5d5a55]">
                <Paperclip className="size-4" />
              </Button>
              <Button
                type="button"
                size="sm"
                onClick={() => void startSession()}
                disabled={!canStart}
                className="rounded-full bg-[#20201f] text-white hover:bg-black disabled:opacity-30"
                aria-label="Start session"
              >
                <ArrowUp className="size-4" />
                <span>Run</span>
              </Button>
            </div>
            {runtime === "cursor" && (
              <div className="grid gap-2 border-t border-black/10 bg-[#f5f4f2] px-4 py-3 sm:grid-cols-[1fr_120px]">
                <Input
                  value={repository}
                  onChange={(event) => setRepository(event.target.value)}
                  placeholder="https://github.com/org/repo"
                  className="h-8 border-black/10 bg-white text-sm"
                />
                <Input
                  value={ref}
                  onChange={(event) => setRef(event.target.value)}
                  placeholder="main"
                  className="h-8 border-black/10 bg-white text-sm"
                />
              </div>
            )}
            {error && (
              <div className="border-t border-red-500/20 bg-red-500/10 px-4 py-3 text-sm text-red-700">
                {error}
              </div>
            )}
          </div>

          <div className="mt-5 grid w-full max-w-2xl gap-3 sm:grid-cols-3">
            <MetricCard title="Sessions" value={sessionCount} />
            <MetricCard title="Saved agents" value={agentCount} />
            <MetricCard
              title="Connected runtimes"
              value={runtimes.filter((item) => item.connected).length}
            />
          </div>

          <div className="absolute bottom-6 rounded-full border border-black/10 bg-white/80 px-3 py-1.5 text-xs text-[#68645f] shadow-sm">
            <span className="mr-2 inline-block size-2 rounded-full bg-[#b7b3ad]" />
            {selectedAgentIsConfigured
              ? `${selectedAgent?.name} ready`
              : selectedRuntime?.connected
                ? `${selectedRuntime.name} ready`
                : "Runtime key missing"}
          </div>
        </section>
      </main>
    </div>
  );
}

export default function SessionsPage() {
  return (
    <Suspense>
      <SessionsStart />
    </Suspense>
  );
}

function MetricCard({
  title,
  value,
}: {
  title: string;
  value: number | null;
}) {
  return (
    <div className="overflow-hidden rounded-lg border border-black/10 bg-white/88 p-4 shadow-sm">
      <div className="text-sm text-[#706c66]">{title}</div>
      <div className="mt-1 text-3xl tracking-tight text-[#20201f]">
        {value === null ? "..." : value.toLocaleString()}
      </div>
      <div className="mt-4 h-1.5 rounded-full bg-black/5">
        <div
          className="h-full rounded-full bg-blue-600/50"
          style={{ width: `${Math.min(100, Math.max(12, (value ?? 0) * 12))}%` }}
        />
      </div>
    </div>
  );
}
