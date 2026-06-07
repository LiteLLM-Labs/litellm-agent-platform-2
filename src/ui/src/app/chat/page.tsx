"use client";

import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useSearchParams, useRouter } from "next/navigation";
import {
  Activity,
  AlertTriangle,
  Bot,
  CheckCircle2,
  ChevronDown,
  Clipboard,
  ClipboardCheck,
  Cpu,
  ExternalLink,
  FileText,
  KeyRound,
  Loader2,
  Square,
  Wrench,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ModelSelect } from "@/components/model-select";
import { MessageBlock } from "@/components/message-block";
import { Composer } from "@/components/composer";
import { ThemeToggle } from "@/components/theme-toggle";
import { Sidebar } from "@/components/sidebar";
import { InspectorPanel } from "@/components/inspector-panel";
import { getMessages, getSession, createSession, deleteSession, subscribeRuntimeEvents, listModels, abortSession, listAgents, listApprovals, acceptApproval, rejectApproval, sendMessageWithRuntimeModel, listRuntimeEvents } from "@/lib/api";
import type { PendingApproval, RuntimeAgentEvent } from "@/lib/api";
import { ToolApprovalPanel } from "@/components/tool-approval-panel";
import type { Agent, AgentRuntimeId, HarnessMessage, HarnessMessagePart } from "@/lib/types";
import type { Frame } from "@/components/inspector-panel";
import SessionsPage from "../sessions/page";

const FALLBACK_MODELS = [
  "anthropic/claude-opus-4-7",
  "anthropic/claude-sonnet-4-5",
  "anthropic/claude-opus-4-1",
  "anthropic/claude-haiku-4-5",
];

const BUILTIN_AGENTS: Record<string, string> = {
  opencode: "OpenCode",
  "claude-code": "Claude Code",
  cc: "Claude Code",
  "github-copilot": "GitHub Copilot",
  codex: "Codex",
};

function agentPrompt(agent: Agent | null): string {
  if (!agent) return "";
  return String(agent.prompt ?? agent.system ?? agent.system_prompt ?? "").trim();
}

function shortPrompt(prompt: string): string {
  const compact = prompt.replace(/\s+/g, " ").trim();
  return compact.length > 220 ? compact.slice(0, 220).trimEnd() + "..." : compact;
}

function runtimeLabel(runtime?: string): string {
  if (runtime === "claude_managed_agents" || runtime === "claude_agents") return "Claude Managed Agents";
  if (runtime === "cursor") return "Cursor";
  return BUILTIN_AGENTS[runtime ?? ""] ?? runtime ?? "Claude Code";
}

function runtimeModelId(runtime?: AgentRuntimeId): string | null {
  if (runtime === "claude_managed_agents") return "anthropic/*";
  if (runtime === "cursor") return "cursor/*";
  if (runtime === "opencode") return "opencode/*";
  return null;
}

function providerSessionUrl(runtime?: string, providerSessionId?: string, providerUrl?: string): string | null {
  if (providerUrl) return providerUrl;
  if ((runtime === "claude_managed_agents" || runtime === "claude_agents") && providerSessionId) {
    return `https://platform.claude.com/workspaces/default/sessions/${encodeURIComponent(providerSessionId)}`;
  }
  return null;
}

function runtimeTextValue(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) {
    return value.map(runtimeTextValue).join("");
  }
  if (!value || typeof value !== "object") return "";
  const record = value as Record<string, unknown>;
  return [
    record.text,
    record.thinking,
    record.content,
    record.delta,
    record.content_block,
  ]
    .map(runtimeTextValue)
    .join("");
}

function runtimeEventText(ev: RuntimeAgentEvent): string {
  return runtimeTextValue(ev.text ?? ev.delta ?? ev.content ?? ev.content_block);
}

function normalizedRuntimeEventType(ev: RuntimeAgentEvent): string {
  const type = ev.type;
  return typeof type === "string" ? type : "";
}

function runtimeEventPartKind(ev: RuntimeAgentEvent): "text" | "thinking" {
  const part = ev.part;
  if (part && typeof part === "object") {
    const type = (part as { type?: unknown }).type;
    if (type === "thinking" || type === "reasoning") return "thinking";
  }
  const field = ev.field;
  if (field === "thinking" || field === "reasoning") return "thinking";
  const type = ev.type;
  if (type === "thinking_back" || type === "agent.thinking" || type === "agent.reasoning") {
    return "thinking";
  }
  return "text";
}

function runtimeErrorMessage(ev: RuntimeAgentEvent): string {
  const error = ev.error;
  if (typeof error === "string") return error;
  if (error && typeof error === "object") {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") return message;
  }
  return JSON.stringify(ev);
}

function isRuntimeAssistantTextEvent(type: string): boolean {
  return (
    type === "assistant_response" ||
    type === "agent.message" ||
    type === "content_block_start" ||
    type === "content_block_delta" ||
    type === "message_delta"
  );
}

function isRuntimeThinkingEvent(type: string): boolean {
  return type === "thinking_back" || type === "agent.thinking" || type === "agent.reasoning";
}

function isRuntimeToolEvent(type: string): boolean {
  return (
    type === "tool_call" ||
    type === "tool_result" ||
    type === "agent.tool_use" ||
    type === "agent.tool_result"
  );
}

function runtimeToolId(ev: RuntimeAgentEvent): string {
  const id = ev.id ?? ev.tool_use_id;
  return typeof id === "string" && id ? id : `tool_${Date.now().toString(36)}`;
}

function optimisticUserMessage(sessionId: string, text: string): HarnessMessage {
  const stamp = Date.now().toString(36);
  const messageId = `${sessionId}_runtime_user_${stamp}`;
  return {
    info: { id: messageId, role: "user", sessionID: sessionId },
    parts: [
      {
        id: `${messageId}_text`,
        messageID: messageId,
        sessionID: sessionId,
        type: "text",
        text,
      },
    ],
  };
}

function messageText(message: HarnessMessage): string {
  return message.parts
    .map((part) => ("text" in part && typeof part.text === "string" ? part.text : ""))
    .join("")
    .trim();
}

function isRuntimeOptimisticUser(message: HarnessMessage, sessionId: string): boolean {
  return (
    message.info.role === "user" &&
    typeof message.info.id === "string" &&
    message.info.id.startsWith(`${sessionId}_runtime_user_`)
  );
}

function mergeServerAndRuntimeMessages(
  serverMessages: HarnessMessage[],
  localMessages: HarnessMessage[],
  sessionId: string,
): HarnessMessage[] {
  const serverIds = new Set(serverMessages.map((message) => message.info.id));
  const localOnly = localMessages.filter((message) => !serverIds.has(message.info.id));
  if (localOnly.length === 0) return serverMessages;

  const insertAfter = new Map<number, HarnessMessage[]>();
  const trailing: HarnessMessage[] = [];
  const consumedServerUsers = new Set<number>();
  let activeServerUserIndex: number | null = null;

  for (const message of localOnly) {
    const runtimeUserText =
      message.info.role === "assistant" && typeof message.info.runtimeUserText === "string"
        ? message.info.runtimeUserText
        : "";
    if (runtimeUserText) {
      const serverIndex = serverMessages.findIndex((serverMessage, index) => (
        !consumedServerUsers.has(index) &&
        serverMessage.info.role === "user" &&
        messageText(serverMessage) === runtimeUserText
      ));
      if (serverIndex === -1) {
        trailing.push(message);
      } else {
        consumedServerUsers.add(serverIndex);
        const items = insertAfter.get(serverIndex) ?? [];
        items.push(message);
        insertAfter.set(serverIndex, items);
      }
      continue;
    }

    if (isRuntimeOptimisticUser(message, sessionId)) {
      const text = messageText(message);
      const serverIndex = serverMessages.findIndex((serverMessage, index) => (
        !consumedServerUsers.has(index) &&
        serverMessage.info.role === "user" &&
        messageText(serverMessage) === text
      ));
      if (serverIndex === -1) {
        activeServerUserIndex = null;
        trailing.push(message);
      } else {
        consumedServerUsers.add(serverIndex);
        activeServerUserIndex = serverIndex;
      }
      continue;
    }

    if (activeServerUserIndex !== null) {
      const items = insertAfter.get(activeServerUserIndex) ?? [];
      items.push(message);
      insertAfter.set(activeServerUserIndex, items);
      continue;
    }

    trailing.push(message);
  }

  const merged: HarnessMessage[] = [];
  serverMessages.forEach((message, index) => {
    merged.push(message);
    const localAfter = insertAfter.get(index);
    if (localAfter) merged.push(...localAfter);
  });
  merged.push(...trailing);
  return merged;
}

function ChatInner() {
  const sp = useSearchParams();
  const sid = sp.get("id");
  const autostartPrompt = sp.get("autostart") === "1" ? sp.get("prompt")?.trim() : "";
  const [messages, setMessages] = useState<HarnessMessage[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [models, setModels] = useState<string[]>(FALLBACK_MODELS);
  const [model, setModel] = useState(FALLBACK_MODELS[0]);
  const [sessionStatus, setSessionStatus] = useState<"idle" | "busy">("idle");
  const [approvals, setApprovals] = useState<PendingApproval[]>([]);
  const [approvalBusy, setApprovalBusy] = useState(false);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [promptCopied, setPromptCopied] = useState(false);
  const eventBufferRef = useRef<Frame[]>([]);
  const seenRuntimeEventIdsRef = useRef<Set<string>>(new Set());
  const runtimeReplayEventCountRef = useRef(0);
  const [sessionHarness, setSessionHarness] = useState<string>("claude-code");
  const [sessionRuntime, setSessionRuntime] = useState<AgentRuntimeId | undefined>();
  const [sessionLoaded, setSessionLoaded] = useState(false);
  const [runtimeHistoryLoaded, setRuntimeHistoryLoaded] = useState(false);
  const [providerSessionId, setProviderSessionId] = useState<string | undefined>();
  const [providerUrl, setProviderUrl] = useState<string | undefined>();
  const [sessionTitle, setSessionTitle] = useState<string>("");
  const [savedAgents, setSavedAgents] = useState<Agent[]>([]);
  const [switchingAgent, setSwitchingAgent] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const wasNearBottomRef = useRef(true);
  const runtimeAssistantRef = useRef<{
    messageId: string;
    textPartId: string;
    thinkingPartId: string;
  } | null>(null);
  const autostartedRef = useRef<string | null>(null);

  const refetch = useCallback(async () => {
    if (!sid) return;
    try {
      const list = await getMessages(sid);
      setMessages((prev) => {
        if (!prev) return list;
        return mergeServerAndRuntimeMessages(list, prev, sid);
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [sid]);

  const router = useRouter();

  const activeAgent = useMemo(() => {
    const target = sessionHarness || sessionTitle;
    return (
      savedAgents.find((a) => a.id === target) ??
      savedAgents.find((a) => a.name === target) ??
      savedAgents.find((a) => sessionTitle && a.name === sessionTitle) ??
      null
    );
  }, [savedAgents, sessionHarness, sessionTitle]);

  const activePrompt = agentPrompt(activeAgent);
  const activeAgentName =
    activeAgent?.name || sessionTitle || BUILTIN_AGENTS[sessionHarness] || sessionHarness;
  const baseRuntime =
    sessionRuntime
      ? runtimeLabel(sessionRuntime)
      : String(activeAgent?.harness ?? activeAgent?.base_agent ?? sessionHarness ?? "claude-code");
  const providerLink = providerSessionUrl(sessionRuntime, providerSessionId, providerUrl);
  const skills = Array.isArray(activeAgent?.skills) ? activeAgent.skills : [];
  const vaultKeys = Array.isArray(activeAgent?.vault_keys) ? activeAgent.vault_keys : [];
  const hasStarted = Boolean(messages && messages.length > 0);
  const modelOptions = useMemo(() => {
    const runtimeModel = runtimeModelId(sessionRuntime);
    return runtimeModel ? [runtimeModel, ...models.filter((item) => item !== runtimeModel)] : models;
  }, [models, sessionRuntime]);

  const onCopyPrompt = useCallback(() => {
    if (!activePrompt) return;
    navigator.clipboard?.writeText(activePrompt).then(() => {
      setPromptCopied(true);
      window.setTimeout(() => setPromptCopied(false), 1400);
    }).catch(() => {});
  }, [activePrompt]);

  useEffect(() => {
    listModels().then((fetched) => {
      if (fetched.length > 0) {
        setModels(fetched);
        setModel((prev) => (fetched.includes(prev) ? prev : fetched[0]));
      }
    }).catch(() => {});
  }, []);

  useEffect(() => {
    const runtimeModel = runtimeModelId(sessionRuntime);
    if (runtimeModel) setModel(runtimeModel);
  }, [models, sessionRuntime]);

  // Fetch session metadata to get the locked agent
  useEffect(() => {
    if (!sid) return;
    seenRuntimeEventIdsRef.current = new Set();
    runtimeReplayEventCountRef.current = 0;
    eventBufferRef.current = [];
    runtimeAssistantRef.current = null;
    setSessionLoaded(false);
    setRuntimeHistoryLoaded(false);
    getSession(sid).then(s => {
      const a = s.agent_id ?? s.agent ?? s.harness;
      if (a) setSessionHarness(a);
      setSessionRuntime(s.runtime);
      setSessionStatus(s.status === "running" ? "busy" : "idle");
      setProviderSessionId(s.provider_session_id);
      setProviderUrl(s.provider_url);
      if (s.title) setSessionTitle(s.title);
    }).catch(() => {}).finally(() => setSessionLoaded(true));
  }, [sid]);

  // Fetch saved agents for dropdown
  useEffect(() => {
    listAgents().then(setSavedAgents).catch(() => {});
  }, []);

  const onHarnessChange = useCallback(async (next: string) => {
    if (!sid || next === sessionHarness) return;
    setSwitchingAgent(true);
    setError(null);
    try {
      if (!hasStarted) await deleteSession(sid).catch(() => {});
      const options = next.startsWith("agent_") && sessionRuntime ? { runtime: sessionRuntime } : undefined;
      const s = await createSession(undefined, next, options);
      router.replace(`/chat/?id=${encodeURIComponent(s.id)}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to switch agent");
      setSwitchingAgent(false);
    }
  }, [hasStarted, sid, sessionHarness, sessionRuntime, router]);

  const runtimeAssistantIds = useCallback(() => {
    if (!sid) return null;
    if (!runtimeAssistantRef.current) {
      const stamp = Date.now().toString(36);
      runtimeAssistantRef.current = {
        messageId: `${sid}_runtime_${stamp}`,
        textPartId: `${sid}_runtime_${stamp}_text`,
        thinkingPartId: `${sid}_runtime_${stamp}_thinking`,
      };
    }
    return runtimeAssistantRef.current;
  }, [sid]);

  const setRuntimeAssistantIds = useCallback((messageId: string) => {
    if (!sid) return null;
    const ids = {
      messageId,
      textPartId: `${messageId}_text`,
      thinkingPartId: `${messageId}_thinking`,
    };
    runtimeAssistantRef.current = ids;
    return ids;
  }, [sid]);

  const ensureRuntimeAssistantMessage = useCallback(() => {
    const ids = runtimeAssistantIds();
    if (!ids) return null;
    setMessages((prev) => {
      const next = prev ?? [];
      if (next.some((m) => m.info.id === ids.messageId)) return next;
      return [
        ...next,
        {
          info: { id: ids.messageId, role: "assistant", sessionID: sid ?? undefined },
          parts: [],
        },
      ];
    });
    return ids;
  }, [runtimeAssistantIds, sid]);

  const finishRuntimeAssistantMessage = useCallback(() => {
    const ids = runtimeAssistantRef.current;
    if (!ids) return;
    setMessages((prev) => {
      if (!prev) return prev;
      const idx = prev.findIndex((m) => m.info.id === ids.messageId);
      if (idx === -1) return prev;
      const next = [...prev];
      const msg = next[idx];
      next[idx] = {
        ...msg,
        info: {
          ...msg.info,
          finish: "stop",
        },
      };
      return next;
    });
    runtimeAssistantRef.current = null;
  }, []);

  const appendRuntimePartText = useCallback((partKind: "text" | "thinking", delta: string) => {
    const ids = runtimeAssistantIds();
    if (!ids || !delta) return;
    const partId = partKind === "thinking" ? ids.thinkingPartId : ids.textPartId;
    setMessages((prev) => {
      let next = prev ?? [];
      let idx = next.findIndex((m) => m.info.id === ids.messageId);
      if (idx === -1) {
        next = [
          ...next,
          {
            info: { id: ids.messageId, role: "assistant", sessionID: sid ?? undefined },
            parts: [
              {
                id: ids.thinkingPartId,
                messageID: ids.messageId,
                sessionID: sid ?? undefined,
                type: "thinking",
                text: "",
              },
              {
                id: ids.textPartId,
                messageID: ids.messageId,
                sessionID: sid ?? undefined,
                type: "text",
                text: "",
              },
            ],
          },
        ];
        idx = next.length - 1;
      } else {
        next = [...next];
      }
      const msg = next[idx];
      let foundPart = false;
      const parts = msg.parts.map((part) => {
        if (part.id !== partId) return part;
        foundPart = true;
        return { ...part, text: `${"text" in part ? part.text : ""}${delta}` } as HarnessMessagePart;
      });
      if (!foundPart) {
        parts.push({
          id: partId,
          messageID: ids.messageId,
          sessionID: sid ?? undefined,
          type: partKind,
          text: delta,
        });
      }
      next[idx] = { ...msg, parts };
      return next;
    });
  }, [runtimeAssistantIds, sid]);

  const appendRuntimeToolEvent = useCallback((ev: RuntimeAgentEvent) => {
    const ids = runtimeAssistantIds();
    if (!ids) return;
    const toolId = runtimeToolId(ev);
    const partId = `${ids.messageId}_${toolId}`;
    const name = typeof ev.name === "string" ? ev.name : "tool";
    const status = typeof ev.status === "string" ? ev.status : ev.type === "tool_result" ? "completed" : "running";
    setMessages((prev) => {
      let next = prev ?? [];
      let idx = next.findIndex((m) => m.info.id === ids.messageId);
      if (idx === -1) {
        next = [
          ...next,
          {
            info: { id: ids.messageId, role: "assistant", sessionID: sid ?? undefined },
            parts: [],
          },
        ];
        idx = next.length - 1;
      } else {
        next = [...next];
      }
      const msg = next[idx];
      let foundPart = false;
      const parts = msg.parts.map((part) => {
        if (part.id !== partId || part.type !== "tool") return part;
        foundPart = true;
        return {
          ...part,
          tool: part.tool || name,
          state: {
            ...part.state,
            status,
            input: part.state.input ?? ev.input,
            output: ev.output ?? part.state.output,
            error: ev.error ?? part.state.error,
          },
        } as HarnessMessagePart;
      });
      if (!foundPart) {
        parts.push({
          id: partId,
          messageID: ids.messageId,
          sessionID: sid ?? undefined,
          type: "tool",
          tool: name,
          state: {
            status,
            input: ev.input,
            output: ev.output,
            error: ev.error,
          },
        });
      }
      next[idx] = { ...msg, parts };
      return next;
    });
  }, [runtimeAssistantIds, sid]);

  const beginRuntimeTurn = useCallback((text?: string) => {
    if (!sessionRuntime || !sid) return;
    runtimeAssistantRef.current = null;
    const ids = runtimeAssistantIds();
    if (!ids) return;
    const trimmed = text?.trim();
    setMessages((prev) => {
      const next = [...(prev ?? [])];
      if (trimmed) {
        next.push(optimisticUserMessage(sid, trimmed));
      }
      next.push({
        info: { id: ids.messageId, role: "assistant", sessionID: sid },
        parts: [],
      });
      return next;
    });
    setSessionStatus("busy");
  }, [runtimeAssistantIds, sessionRuntime, sid]);

  const beginRuntimeReplayTurn = useCallback((ev: RuntimeAgentEvent) => {
    if (!sid) return;
    const userText = runtimeTextValue(ev.content).trim();
    const eventId = typeof ev.id === "string" && ev.id ? ev.id : Date.now().toString(36);
    const ids = setRuntimeAssistantIds(`${sid}_runtime_${eventId}`);
    if (!ids) return;

    setMessages((prev) => {
      const next = [...(prev ?? [])];
      let userIndex = -1;
      if (userText) {
        for (let index = 0; index < next.length; index += 1) {
          const message = next[index];
          if (message.info.role !== "user" || messageText(message) !== userText) continue;
          const nextUserIndex = next.findIndex(
            (candidate, candidateIndex) => candidateIndex > index && candidate.info.role === "user",
          );
          const searchEnd = nextUserIndex === -1 ? next.length : nextUserIndex;
          const hasAssistant = next
            .slice(index + 1, searchEnd)
            .some((candidate) => candidate.info.role === "assistant");
          if (!hasAssistant) {
            userIndex = index;
            break;
          }
          if (userIndex === -1) userIndex = index;
        }
      }

      const existingIndex = next.findIndex((message) => message.info.id === ids.messageId);
      if (existingIndex !== -1) return next;

      const assistantMessage: HarnessMessage = {
        info: { id: ids.messageId, role: "assistant", sessionID: sid, runtimeUserText: userText },
        parts: [],
      };
      if (userIndex === -1) return [...next, assistantMessage];
      return [
        ...next.slice(0, userIndex + 1),
        assistantMessage,
        ...next.slice(userIndex + 1),
      ];
    });
    setSessionStatus("busy");
  }, [setRuntimeAssistantIds, sid]);

  useEffect(() => {
    if (!sid || !sessionRuntime || !runtimeHistoryLoaded || sessionStatus !== "busy" || !messages) return;
    const hasPendingAssistant = messages.some((message) => {
      if (message.info.role !== "assistant") return false;
      if (message.info.finish) return false;
      return message.parts.length === 0;
    });
    let lastUserIndex = -1;
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      if (messages[index].info.role === "user") {
        lastUserIndex = index;
        break;
      }
    }
    const hasAssistantAfterLastUser =
      lastUserIndex !== -1 &&
      messages.slice(lastUserIndex + 1).some((message) => message.info.role === "assistant");
    if (!hasPendingAssistant && !hasAssistantAfterLastUser) {
      ensureRuntimeAssistantMessage();
    }
  }, [ensureRuntimeAssistantMessage, messages, runtimeHistoryLoaded, sessionRuntime, sessionStatus, sid]);

  const handleRuntimeEvent = useCallback((ev: RuntimeAgentEvent) => {
    const eventId = typeof ev.id === "string" ? ev.id : "";
    if (eventId) {
      if (seenRuntimeEventIdsRef.current.has(eventId)) return;
      seenRuntimeEventIdsRef.current.add(eventId);
    }

    eventBufferRef.current = [
      ...eventBufferRef.current.slice(-499),
      { ts: Date.now(), ev: ev as Frame["ev"] },
    ];

    const type = normalizedRuntimeEventType(ev);

    if (type === "user.message") {
      beginRuntimeReplayTurn(ev);
      return;
    }

    if (type === "session.status_running") {
      setSessionStatus("busy");
      return;
    }

    if (type === "session.status") {
      const status = ev.status;
      const statusType =
        typeof status === "string"
          ? status
          : status && typeof status === "object"
            ? (status as { type?: unknown }).type
            : undefined;
      if (statusType === "busy" || statusType === "running") {
        ensureRuntimeAssistantMessage();
        setSessionStatus("busy");
      }
      if (statusType === "idle") {
        setSessionStatus("idle");
        finishRuntimeAssistantMessage();
      }
      return;
    }

    if (type === "session.status_idle") {
      setSessionStatus("idle");
      finishRuntimeAssistantMessage();
      return;
    }

    if (type === "session.error") {
      setError(`Error: ${runtimeErrorMessage(ev)}`);
      setSessionStatus("idle");
      runtimeAssistantRef.current = null;
      return;
    }

    if (isRuntimeToolEvent(type)) {
      ensureRuntimeAssistantMessage();
      appendRuntimeToolEvent(ev);
      setSessionStatus("busy");
      return;
    }

    if (!isRuntimeAssistantTextEvent(type) && !isRuntimeThinkingEvent(type)) return;
    ensureRuntimeAssistantMessage();
    const delta = runtimeEventText(ev);
    if (delta) {
      appendRuntimePartText(isRuntimeThinkingEvent(type) ? "thinking" : runtimeEventPartKind(ev), delta);
    }
    setSessionStatus("busy");
  }, [appendRuntimePartText, appendRuntimeToolEvent, beginRuntimeReplayTurn, ensureRuntimeAssistantMessage, finishRuntimeAssistantMessage]);

  const replayRuntimeEvents = useCallback((events: RuntimeAgentEvent[]) => {
    const start = Math.min(runtimeReplayEventCountRef.current, events.length);
    runtimeReplayEventCountRef.current = events.length;
    events.slice(start).forEach(handleRuntimeEvent);
  }, [handleRuntimeEvent]);

  useEffect(() => {
    if (!sid || !sessionLoaded) return;
    refetch();
    let unsub: (() => void) | undefined;
    if (sessionRuntime) {
      listRuntimeEvents(sid)
        .then((events) => {
          replayRuntimeEvents(events);
          setRuntimeHistoryLoaded(true);
        })
        .catch((err) => {
          setRuntimeHistoryLoaded(true);
          setError(err instanceof Error ? err.message : String(err));
        });
      if (sessionStatus === "busy" || autostartPrompt) {
        unsub = subscribeRuntimeEvents({
          sessionId: sid,
          onEvent: handleRuntimeEvent,
          onError: (err) => setError(err instanceof Error ? err.message : String(err)),
        });
      }
    }
    if (autostartPrompt && autostartedRef.current !== sid) {
      autostartedRef.current = sid;
      beginRuntimeTurn(autostartPrompt);
      void sendMessageWithRuntimeModel({
        sessionId: sid,
        text: autostartPrompt,
        model,
        runtime: sessionRuntime,
      })
        .then(() => {
          if (!sessionRuntime) return refetch();
        })
        .then(() => router.replace(`/chat/?id=${encodeURIComponent(sid)}`))
        .catch((err) => {
          setError(err instanceof Error ? err.message : String(err));
          setSessionStatus("idle");
          runtimeAssistantRef.current = null;
        });
    }
    listApprovals().then(setApprovals).catch(() => {});
    return unsub;
  }, [sid, sessionLoaded, refetch, handleRuntimeEvent, replayRuntimeEvents, autostartPrompt, beginRuntimeTurn, model, router, sessionRuntime, sessionStatus]);

  useEffect(() => {
    if (!sid || !sessionRuntime || sessionStatus !== "busy") return;
    let active = true;
    const replay = () => {
      listRuntimeEvents(sid)
        .then((events) => {
          if (!active) return;
          replayRuntimeEvents(events);
        })
        .catch((err) => {
          if (active) setError(err instanceof Error ? err.message : String(err));
        });
    };
    replay();
    const timer = window.setInterval(replay, 2000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [sid, sessionRuntime, sessionStatus, replayRuntimeEvents]);

  const onApprovalAccept = useCallback(async (id: string, args: Record<string, unknown>) => {
    setApprovalBusy(true);
    try {
      await acceptApproval(id, args);
      setApprovals((prev) => prev.filter((a) => a.id !== id));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setApprovalBusy(false);
    }
  }, []);

  const onApprovalReject = useCallback(async (id: string, feedback: string) => {
    setApprovalBusy(true);
    try {
      await rejectApproval(id, feedback);
      setApprovals((prev) => prev.filter((a) => a.id !== id));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setApprovalBusy(false);
    }
  }, []);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const dist = el.scrollHeight - (el.scrollTop + el.clientHeight);
    wasNearBottomRef.current = dist < 120;
  };

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    if (wasNearBottomRef.current) el.scrollTop = el.scrollHeight;
  }, [messages]);

  if (!sid) {
    return <SessionsPage />;
  }

  const shortSid = sid.length > 12 ? sid.slice(0, 12) + "…" : sid;

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar activeId={sid} />

      <div className="flex-1 flex flex-col min-w-0">
        <header className="h-12 border-b border-border flex items-center justify-between px-4 shrink-0">
          <div className="flex items-center gap-2">
            {sessionTitle && (
              <span className="text-sm font-medium" title={sessionTitle}>{sessionTitle}</span>
            )}
            <span className="text-xs font-mono text-muted-foreground">{shortSid}</span>
            {sessionStatus === "busy" ? (
              <button
                onClick={() => sid && abortSession(sid).catch(() => {})}
                className="flex items-center gap-1 text-[11px] text-amber-500 font-mono hover:text-red-500 transition-colors group"
                title="Abort agent"
              >
                <Loader2 className="w-3 h-3 animate-spin group-hover:hidden" />
                <Square className="w-3 h-3 hidden group-hover:block fill-current" />
                <span className="group-hover:hidden">busy</span>
                <span className="hidden group-hover:inline">abort</span>
              </button>
            ) : (
              <span className="flex items-center gap-1 text-[11px] text-emerald-500 font-mono">
                <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 inline-block" />
                idle
              </span>
            )}
          </div>
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-1.5">
              <span className="text-[11px] text-muted-foreground">agent</span>
              <Select
                value={sessionHarness}
                onValueChange={(v) => v && onHarnessChange(v)}
                disabled={switchingAgent || sessionStatus === "busy"}
              >
                <SelectTrigger className="h-8 text-xs w-[190px]">
                  <SelectValue placeholder={activeAgentName} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="opencode" className="text-xs font-mono">opencode</SelectItem>
                  <SelectItem value="claude-code" className="text-xs font-mono">claude code</SelectItem>
                  <SelectItem value="github-copilot" className="text-xs font-mono">github copilot</SelectItem>
                  {savedAgents.length > 0 && (
                    <>
                      <div className="px-2 py-1.5 text-[10px] text-muted-foreground uppercase tracking-wider border-t mt-1 pt-2">Saved agents</div>
                      {savedAgents.map(a => (
                        <SelectItem key={a.id} value={a.id} className="text-xs font-mono">{a.name}</SelectItem>
                      ))}
                    </>
                  )}
                  <div className="px-2 py-2 text-[10px] text-muted-foreground border-t mt-1">
                    Switching agents opens a new session.
                  </div>
                </SelectContent>
              </Select>
              {switchingAgent && <Loader2 className="size-3.5 animate-spin text-muted-foreground" />}
            </div>
            <div className="flex items-center gap-1.5">
              <span className="text-[11px] text-muted-foreground">model</span>
              <ModelSelect value={model} models={modelOptions} onValueChange={setModel} />
            </div>
            {providerLink && (
              <Button
                variant="outline"
                size="sm"
                className="h-8"
                render={
                  <a href={providerLink} target="_blank" rel="noreferrer">
                    <ExternalLink className="size-3.5" />
                    Open provider session
                  </a>
                }
              />
            )}
            <Button
              variant={inspectorOpen ? "default" : "outline"}
              size="sm"
              onClick={() => setInspectorOpen((v) => !v)}
              className="h-8"
            >
              <Activity className="size-3.5" />
              Inspect
            </Button>
            <ThemeToggle />
          </div>
        </header>

        <div
          ref={scrollRef}
          onScroll={onScroll}
          className="flex-1 overflow-y-auto"
        >
          <div className="mx-auto flex w-full max-w-5xl flex-col gap-6 px-6 py-8">
            {!messages && !error && (
              <div className="text-muted-foreground text-sm">Loading…</div>
            )}
            {error && (
              <Card className="border-destructive p-4">
                <p className="text-sm text-destructive">{error}</p>
              </Card>
            )}
            <Card className="gap-0 overflow-hidden rounded-lg border border-border/80 bg-card/80 py-0 ring-0">
              <div className="grid gap-0 md:grid-cols-[minmax(0,1fr)_minmax(280px,360px)]">
                <section className="min-w-0 border-b border-border/70 p-4 md:border-b-0 md:border-r">
                  <div className="flex items-start gap-3">
                    <div className="flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-background">
                      <Bot className="size-4 text-foreground" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <h2 className="truncate text-sm font-semibold leading-5">{activeAgentName}</h2>
                        {activePrompt ? (
                          <span className="inline-flex h-5 items-center gap-1 rounded-md border border-emerald-500/25 bg-emerald-500/10 px-1.5 text-[10px] font-medium text-emerald-500">
                            <CheckCircle2 className="size-3" />
                            prompt active
                          </span>
                        ) : (
                          <span className="inline-flex h-5 items-center gap-1 rounded-md border border-amber-500/25 bg-amber-500/10 px-1.5 text-[10px] font-medium text-amber-500">
                            <AlertTriangle className="size-3" />
                            no saved prompt
                          </span>
                        )}
                      </div>
                      {activeAgent?.description ? (
                        <p className="mt-1 max-w-2xl text-xs leading-relaxed text-muted-foreground">
                          {String(activeAgent.description)}
                        </p>
                      ) : (
                        <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                          Session instructions and runtime context are shown before the transcript.
                        </p>
                      )}
                      <div className="mt-3 grid gap-1.5 text-[11px] sm:grid-cols-2">
                        <div className="flex min-w-0 items-center gap-1.5 rounded-md border border-border/70 bg-background px-2 py-1.5">
                          <Cpu className="size-3.5 shrink-0 text-muted-foreground" />
                          <span className="text-muted-foreground">runtime</span>
                          <span className="ml-auto truncate font-mono text-foreground">{baseRuntime}</span>
                        </div>
                        <div className="flex min-w-0 items-center gap-1.5 rounded-md border border-border/70 bg-background px-2 py-1.5">
                          <FileText className="size-3.5 shrink-0 text-muted-foreground" />
                          <span className="text-muted-foreground">session</span>
                          <span className="ml-auto truncate font-mono text-foreground">{shortSid}</span>
                        </div>
                        {providerLink && (
                          <a
                            href={providerLink}
                            target="_blank"
                            rel="noreferrer"
                            className="flex min-w-0 items-center gap-1.5 rounded-md border border-border/70 bg-background px-2 py-1.5 hover:bg-muted"
                          >
                            <ExternalLink className="size-3.5 shrink-0 text-muted-foreground" />
                            <span className="text-muted-foreground">provider</span>
                            <span className="ml-auto truncate font-mono text-foreground">
                              {providerSessionId ?? "open"}
                            </span>
                          </a>
                        )}
                      </div>
                      {(skills.length > 0 || vaultKeys.length > 0) && (
                        <div className="mt-3 flex flex-wrap gap-1.5">
                          {skills.map((skill) => (
                            <span
                              key={skill}
                              className="inline-flex h-5 items-center gap-1 rounded-md border border-sky-500/25 bg-sky-500/10 px-1.5 font-mono text-[10px] text-sky-500"
                            >
                              <Wrench className="size-3" />
                              {skill}
                            </span>
                          ))}
                          {vaultKeys.map((key) => (
                            <span
                              key={key}
                              className="inline-flex h-5 items-center gap-1 rounded-md border border-amber-500/25 bg-amber-500/10 px-1.5 font-mono text-[10px] text-amber-500"
                            >
                              <KeyRound className="size-3" />
                              {key}
                            </span>
                          ))}
                        </div>
                      )}
                    </div>
                  </div>
                </section>

                <section className="min-w-0 bg-background/35 p-4">
                  <div className="flex items-center gap-2">
                    <div className="min-w-0">
                      <div className="text-xs font-medium">System prompt</div>
                      <div className="text-[11px] text-muted-foreground">
                        {activePrompt ? "Visible before the first turn runs." : "No reusable agent prompt is attached."}
                      </div>
                    </div>
                    <div className="ml-auto flex shrink-0 items-center gap-1">
                      <Button
                        type="button"
                        variant="outline"
                        size="icon-sm"
                        disabled={!activePrompt}
                        onClick={onCopyPrompt}
                        aria-label="Copy system prompt"
                        title="Copy system prompt"
                      >
                        {promptCopied ? <ClipboardCheck className="size-3.5" /> : <Clipboard className="size-3.5" />}
                      </Button>
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="h-7"
                        onClick={() => setPromptOpen((v) => !v)}
                        disabled={!activePrompt}
                        title={promptOpen ? "Collapse system prompt" : "Expand system prompt"}
                      >
                        <span>{promptOpen ? "Full" : "Preview"}</span>
                        <ChevronDown className={`size-3.5 transition-transform ${promptOpen ? "rotate-180" : ""}`} />
                      </Button>
                    </div>
                  </div>
                  <div className="mt-3">
                    {activePrompt ? (
                      promptOpen ? (
                        <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-md border border-border bg-background p-3 font-mono text-[12px] leading-relaxed text-foreground">
                          {activePrompt}
                        </pre>
                      ) : (
                        <div className="rounded-md border border-border bg-background p-3">
                          <p className="line-clamp-4 font-mono text-[12px] leading-relaxed text-muted-foreground">
                            {shortPrompt(activePrompt)}
                          </p>
                        </div>
                      )
                    ) : (
                      <div className="rounded-md border border-amber-500/25 bg-amber-500/10 p-3 text-xs leading-relaxed text-amber-500">
                        {activeAgent
                          ? "This saved agent will run without a stored system prompt until one is added on the Agents page."
                          : "This is a built-in runtime session, so there is no saved agent prompt to review."}
                      </div>
                    )}
                  </div>
                  {promptCopied && (
                    <div className="mt-2 text-[11px] text-emerald-500">
                      Copied system prompt.
                    </div>
                  )}
                </section>
              </div>
            </Card>
            {messages && messages.length === 0 && (
              <div className="py-16 text-center text-sm text-muted-foreground">
                No messages yet. Say hi.
              </div>
            )}
            {messages?.map((m, i) => (
              <MessageBlock
                key={(m.info.id as string | undefined) ?? i}
                msg={m}
              />
            ))}
            {approvals.map((a) => (
              <ToolApprovalPanel
                key={a.id}
                approval={a}
                onAccept={onApprovalAccept}
                onReject={onApprovalReject}
                busy={approvalBusy}
              />
            ))}
          </div>
        </div>

        <Composer
          sessionId={sid}
          model={model}
          onSent={sessionRuntime ? undefined : refetch}
          onSend={sessionRuntime ? (text) => sendMessageWithRuntimeModel({
            sessionId: sid,
            text,
            model,
            runtime: sessionRuntime,
          }) : undefined}
          onSendStart={beginRuntimeTurn}
          disabled={Boolean(sessionRuntime && sessionStatus === "busy")}
        />
      </div>

      <InspectorPanel
        open={inspectorOpen}
        onClose={() => setInspectorOpen(false)}
        sessionId={sid}
        initialFrames={eventBufferRef.current}
      />
    </div>
  );
}

export default function ChatPage() {
  return (
    <Suspense
      fallback={
        <div className="min-h-screen flex items-center justify-center text-muted-foreground text-sm">
          Loading…
        </div>
      }
    >
      <ChatInner />
    </Suspense>
  );
}
