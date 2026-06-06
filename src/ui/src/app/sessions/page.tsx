"use client";

import { useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { ArrowUp, Bot, Mic, Paperclip } from "lucide-react";
import { BrandIcon } from "@/components/brand-icons";
import { Sidebar } from "@/components/sidebar";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { createAgent, createSession, listAgentRuntimes, listAgents, listSessions } from "@/lib/api";
import type { Agent, AgentRuntime, AgentRuntimeId } from "@/lib/types";

const CLAUDE_RUNTIME: AgentRuntimeId = "claude_managed_agents";
const AGENT_LAUNCHER_PREFIX = "agent:";
const RUNTIME_LAUNCHER_PREFIX = "runtime:";

function isAgentRuntimeId(value: string): value is AgentRuntimeId {
  return value === "claude_managed_agents" || value === "cursor" || value === "opencode";
}

function agentLauncherValue(agentId: string): string {
  return `${AGENT_LAUNCHER_PREFIX}${agentId}`;
}

function runtimeLauncherValue(runtime: AgentRuntimeId): string {
  return `${RUNTIME_LAUNCHER_PREFIX}${runtime}`;
}

function launcherAgentId(value: string): string {
  return value.startsWith(AGENT_LAUNCHER_PREFIX) ? value.slice(AGENT_LAUNCHER_PREFIX.length) : "";
}

function launcherRuntimeId(value: string): AgentRuntimeId | "" {
  if (!value.startsWith(RUNTIME_LAUNCHER_PREFIX)) return "";
  const id = value.slice(RUNTIME_LAUNCHER_PREFIX.length);
  return isAgentRuntimeId(id) ? id : "";
}

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

function runtimeFromAgent(agent: Agent | null | undefined): AgentRuntimeId | "" {
  const harness = String(agent?.harness ?? "");
  if (harness === "claude_agents") return "claude_managed_agents";
  return isAgentRuntimeId(harness) ? harness : "";
}

function agentLauncherLabel(agent: Agent): string {
  const runtime = runtimeFromAgent(agent);
  return runtime ? runtimeLabel(runtime) : runtimeLabel(String(agent.harness ?? "claude-code"));
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

function cursorEnvironment(repository: string, ref: string): Record<string, unknown> {
  const repo = repository.trim();
  if (!repo) return {};
  return {
    repository: repo,
    ref: ref.trim() || "main",
    target_branch: "agent/{agent_id}/{session_id}",
    auto_create_pr: false,
  };
}

export default function SessionsPage() {
  const router = useRouter();
  const [prompt, setPrompt] = useState("");
  const [launcher, setLauncher] = useState("");
  const [runtimes, setRuntimes] = useState<AgentRuntime[]>([]);
  const [agents, setAgents] = useState<Agent[]>([]);
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
        setAgents(nextAgents);
        setLauncher((current) => {
          const currentAgent = launcherAgentId(current);
          if (currentAgent && nextAgents.some((item) => item.id === currentAgent)) return current;

          const currentRuntime = launcherRuntimeId(current);
          if (currentRuntime && nextRuntimes.some((item) => item.id === currentRuntime)) return current;

          const firstAgent = nextAgents[0];
          if (firstAgent) return agentLauncherValue(firstAgent.id);

          const fallbackRuntime =
            nextRuntimes.find((item) => item.id === CLAUDE_RUNTIME)?.id ?? nextRuntimes[0]?.id;
          return fallbackRuntime ? runtimeLauncherValue(fallbackRuntime) : "";
        });
        setSessionCount(nextSessions.length);
        setAgentCount(nextAgents.length);
      })
      .catch((err) => setError(err instanceof Error ? err.message : "Failed to load runtimes"));
  }, []);

  const selectedAgentId = launcherAgentId(launcher);
  const selectedAgent = useMemo(
    () => agents.find((item) => item.id === selectedAgentId) ?? null,
    [agents, selectedAgentId],
  );
  const selectedRuntimeId = selectedAgent ? runtimeFromAgent(selectedAgent) : launcherRuntimeId(launcher);
  const selectedRuntime = useMemo(
    () => selectedRuntimeId ? runtimes.find((item) => item.id === selectedRuntimeId) : undefined,
    [selectedRuntimeId, runtimes],
  );
  const selectedRuntimeReady = selectedRuntimeId ? selectedRuntime?.connected === true : true;
  const needsRepository = !selectedAgent && selectedRuntimeId === "cursor";
  const canStart =
    launcher !== "" &&
    prompt.trim().length > 0 &&
    !starting &&
    selectedRuntimeReady &&
    (!needsRepository || repository.trim().length > 0);

  const startSession = async () => {
    const trimmed = prompt.trim();
    const runtimeId = selectedRuntimeId;
    if (!canStart) return;
    setStarting(true);
    setError(null);
    try {
      const title = promptTitle(trimmed);
      const agent =
        selectedAgent ??
        (runtimeId
          ? await createAgent({
              name: title,
              owner_id: "default",
              description: `Started from ${runtimeLabel(selectedRuntime ?? runtimeId)} landing prompt.`,
              model: modelForRuntime(runtimeId),
              harness: runtimeId,
              system: "You are a helpful managed agent. Use available tools when they help complete the user's request.",
              tools: [{ type: "agent_toolset_20260401" }],
              mcp_servers: [],
              skills: [],
            })
          : null);
      if (!agent) return;

      const environment = runtimeId === "cursor" ? cursorEnvironment(repository, ref) : {};
      const session = await createSession(
        title,
        agent.id,
        runtimeId
          ? {
              runtime: runtimeId,
              environment,
            }
          : undefined,
      );
      const params = new URLSearchParams({
        id: session.id,
        prompt: trimmed,
        autostart: "1",
      });
      router.push(`/chat/?${params.toString()}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to start session");
    } finally {
      setStarting(false);
    }
  };
  const launcherName =
    selectedAgent?.name ??
    selectedRuntime?.name ??
    (selectedRuntimeId ? runtimeLabel(selectedRuntimeId) : "Select agent");
  const launcherRoute = selectedAgent && !selectedRuntimeId ? "agent/*" : runtimeRoutePrefix(selectedRuntimeId);
  const launcherStatus =
    selectedAgent && (!selectedRuntimeId || selectedRuntime?.connected)
      ? `${selectedAgent.name} ready`
      : selectedRuntime?.connected
        ? `${selectedRuntime.name} ready`
        : selectedRuntimeId
          ? "Runtime key missing"
          : "Select an agent";

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
              placeholder="Ask or build anything"
              className="min-h-24 resize-none border-0 bg-transparent px-4 py-4 text-[15px] text-[#20201f] shadow-none outline-none placeholder:text-[#77736d] focus-visible:ring-0"
            />
            <div className="flex flex-wrap items-center gap-2 border-t border-black/10 bg-[#faf9f7] px-3 py-3">
              <Select value={launcher} onValueChange={(value) => value && setLauncher(value)}>
                <SelectTrigger className="h-10 w-auto min-w-[250px] rounded-full border border-black/10 bg-white px-3 text-left text-[#20201f] shadow-sm transition-colors hover:bg-[#fbfaf8] focus:ring-1 focus:ring-black/15">
                  <SelectValue>
                    <span className="flex min-w-0 items-center gap-2">
                      <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-[#f3f1ee]">
                        {selectedAgent ? (
                          <Bot className="size-4 text-[#4b4843]" />
                        ) : (
                          <BrandIcon id={runtimeIconId(selectedRuntimeId)} className="size-4" />
                        )}
                      </span>
                      <span className="truncate text-sm font-medium">
                        {launcherName}
                      </span>
                    </span>
                  </SelectValue>
                </SelectTrigger>
                <SelectContent className="w-[340px]">
                  {agents.length > 0 && (
                    <>
                      <SelectGroup>
                        <SelectLabel>Saved agents</SelectLabel>
                        {agents.map((agent) => {
                          const agentRuntime = runtimeFromAgent(agent);
                          const runtimeConfig = agentRuntime
                            ? runtimes.find((item) => item.id === agentRuntime)
                            : undefined;
                          const disabled = agentRuntime ? runtimeConfig?.connected !== true : false;
                          return (
                            <SelectItem
                              key={agent.id}
                              value={agentLauncherValue(agent.id)}
                              disabled={disabled}
                              className="py-3"
                            >
                              <span className="flex min-w-0 items-center gap-3">
                                <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-border bg-background">
                                  <Bot className="size-4" />
                                </span>
                                <span className="min-w-0">
                                  <span className="block truncate text-sm font-medium">
                                    {String(agent.name)}
                                  </span>
                                  <span className="block truncate text-xs text-muted-foreground">
                                    {disabled
                                      ? `${runtimeLabel(agentRuntime)} missing key`
                                      : String(agent.description ?? agentLauncherLabel(agent))}
                                  </span>
                                </span>
                              </span>
                            </SelectItem>
                          );
                        })}
                      </SelectGroup>
                      <SelectSeparator />
                    </>
                  )}
                  <SelectGroup>
                    <SelectLabel>Start from runtime</SelectLabel>
                    {runtimes.map((item) => {
                      return (
                        <SelectItem
                          key={item.id}
                          value={runtimeLauncherValue(item.id)}
                          disabled={!item.connected}
                          className="py-3"
                        >
                          <span className="flex min-w-0 items-center gap-3">
                            <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-border bg-background">
                              <BrandIcon id={runtimeIconId(item.id)} className="size-4" />
                            </span>
                            <span className="min-w-0">
                              <span className="block truncate text-sm font-medium">
                                {item.name}
                              </span>
                              <span className="block truncate text-xs text-muted-foreground">
                                {runtimeSubtitle(item)}
                              </span>
                            </span>
                          </span>
                        </SelectItem>
                      );
                    })}
                  </SelectGroup>
                </SelectContent>
              </Select>
              <span className="hidden rounded-full border border-black/10 bg-white px-3 py-1.5 font-mono text-xs text-[#77736d] sm:inline">
                {launcherRoute}
              </span>
              <div className="ml-auto" />
              <Button variant="ghost" size="icon-sm" disabled className="text-[#5d5a55]">
                <Mic className="size-4" />
              </Button>
              <Button variant="ghost" size="icon-sm" disabled className="text-[#5d5a55]">
                <Paperclip className="size-4" />
              </Button>
              <Button
                type="button"
                size="icon-sm"
                onClick={() => void startSession()}
                disabled={!canStart}
                className="rounded-full bg-[#20201f] text-white hover:bg-black disabled:opacity-30"
                aria-label="Start session"
              >
                <ArrowUp className="size-4" />
              </Button>
            </div>
            {selectedRuntimeId === "cursor" && (
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
            {launcherStatus}
          </div>
        </section>
      </main>
    </div>
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
