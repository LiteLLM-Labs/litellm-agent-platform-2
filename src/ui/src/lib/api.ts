import type {
  Agent,
  AgentFile,
  AgentRunStart,
  AgentRuntime,
  AgentRuntimeId,
  HarnessMessage,
  Memory,
  OpencodeSession,
  Skill,
  SpendLog,
} from "./types";

const BASE = "";
const MASTER_KEY_STORAGE = "lite-harness-master-key";
const HARNESS_SERVER_URL_STORAGE = "lite-harness-server-url";
const HARNESS_SERVER_KEY_STORAGE = "lite-harness-server-key";

export class ApiError extends Error {
  status: number;
  body: string;
  constructor(status: number, body: string, message?: string) {
    super(message ?? `HTTP ${status}: ${body}`);
    this.status = status;
    this.body = body;
  }
}

export function getStoredMasterKey(): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.sessionStorage.getItem(MASTER_KEY_STORAGE);
  } catch {
    return null;
  }
}

export function setStoredMasterKey(key: string): void {
  if (typeof window === "undefined") return;
  try {
    window.sessionStorage.setItem(MASTER_KEY_STORAGE, key);
  } catch {
    /* noop */
  }
}

export function clearStoredMasterKey(): void {
  if (typeof window === "undefined") return;
  try {
    window.sessionStorage.removeItem(MASTER_KEY_STORAGE);
  } catch {
    /* noop */
  }
}

export function normalizeHarnessServerUrl(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  try {
    const url = new URL(trimmed.includes("://") ? trimmed : `http://${trimmed}`);
    if (url.protocol !== "http:" && url.protocol !== "https:") return "";
    url.hash = "";
    url.search = "";
    return url.toString().replace(/\/+$/, "");
  } catch {
    return "";
  }
}

export function getHarnessServerUrl(): string {
  if (typeof window === "undefined") return "";
  try {
    return normalizeHarnessServerUrl(
      window.localStorage.getItem(HARNESS_SERVER_URL_STORAGE) ?? "",
    );
  } catch {
    return "";
  }
}

export function setHarnessServerUrl(value: string): string {
  const normalized = normalizeHarnessServerUrl(value);
  if (typeof window === "undefined") return normalized;
  try {
    if (normalized) window.localStorage.setItem(HARNESS_SERVER_URL_STORAGE, normalized);
    else window.localStorage.removeItem(HARNESS_SERVER_URL_STORAGE);
  } catch {
    /* noop */
  }
  return normalized;
}

export function clearHarnessServerUrl(): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.removeItem(HARNESS_SERVER_URL_STORAGE);
  } catch {
    /* noop */
  }
}

export function getHarnessServerKey(): string {
  if (typeof window === "undefined") return "";
  try {
    return window.sessionStorage.getItem(HARNESS_SERVER_KEY_STORAGE) ?? "";
  } catch {
    return "";
  }
}

export function setHarnessServerKey(value: string): void {
  if (typeof window === "undefined") return;
  try {
    const trimmed = value.trim();
    if (trimmed) window.sessionStorage.setItem(HARNESS_SERVER_KEY_STORAGE, trimmed);
    else window.sessionStorage.removeItem(HARNESS_SERVER_KEY_STORAGE);
  } catch {
    /* noop */
  }
}

export function clearHarnessServerKey(): void {
  if (typeof window === "undefined") return;
  try {
    window.sessionStorage.removeItem(HARNESS_SERVER_KEY_STORAGE);
  } catch {
    /* noop */
  }
}

function withAuth(init?: RequestInit): RequestInit {
  const key = getStoredMasterKey();
  if (!key) return { cache: "no-store", ...init };
  const headers = new Headers(init?.headers);
  if (!headers.has("authorization")) headers.set("authorization", `Bearer ${key}`);
  return { cache: "no-store", ...init, headers };
}

async function req(path: string, init?: RequestInit): Promise<Response> {
  const res = await fetch(BASE + path, withAuth(init));
  if (res.status === 401 && typeof window !== "undefined") {
    clearStoredMasterKey();
    const next = encodeURIComponent(window.location.pathname + window.location.search);
    if (!window.location.pathname.startsWith("/login")) {
      window.location.replace(`/login/?next=${next}`);
    }
  }
  return res;
}

function harnessProxyPath(path: string, base = getHarnessServerUrl()): string {
  const cleanPath = path.replace(/^\/+/, "");
  const qs = new URLSearchParams({ base });
  return `${BASE}/api/harness-proxy/${cleanPath}?${qs.toString()}`;
}

function withHarnessProxyAuth(init?: RequestInit, targetKey = getHarnessServerKey()): RequestInit {
  const headers = new Headers(init?.headers);
  const key = getStoredMasterKey();
  if (key && !headers.has("authorization")) headers.set("authorization", `Bearer ${key}`);
  if (targetKey.trim()) headers.set("x-lite-harness-target-key", targetKey.trim());
  return { cache: "no-store", ...init, headers };
}

async function reqHarness(path: string, init?: RequestInit): Promise<Response> {
  const base = getHarnessServerUrl();
  if (!base) return req(path, init);
  return fetch(harnessProxyPath(path, base), withHarnessProxyAuth(init));
}

export async function whoami(): Promise<void> {
  const res = await req("/v1/models");
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new ApiError(res.status, body);
  }
}

async function jsonOrThrow<T>(res: Response): Promise<T> {
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new ApiError(res.status, body);
  }
  return (await res.json()) as T;
}

export async function listSessions(): Promise<OpencodeSession[]> {
  const res = await reqHarness("/session");
  if (!res.headers.get("content-type")?.includes("application/json")) return [];
  const list = await jsonOrThrow<OpencodeSession[]>(res);
  return [...list].sort(
    (a, b) => (b.time?.created ?? 0) - (a.time?.created ?? 0),
  );
}

export async function createSession(
  title?: string,
  agent?: string,
  options?: {
    runtime?: AgentRuntimeId;
    prompt?: string;
    environment?: Record<string, unknown>;
  },
): Promise<OpencodeSession> {
  const res = await reqHarness("/session", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      title,
      ...(agent ? { agent, agent_id: agent, harness: agent } : {}),
      ...(options?.runtime ? { runtime: options.runtime } : {}),
      ...(options?.prompt ? { prompt: options.prompt } : {}),
      ...(options?.environment ? { environment: options.environment } : {}),
    }),
  });
  return jsonOrThrow<OpencodeSession>(res);
}

export async function listAgentRuntimes(): Promise<AgentRuntime[]> {
  const res = await req("/api/agent-runtimes");
  const data = await jsonOrThrow<{ runtimes: AgentRuntime[] }>(res);
  return data.runtimes;
}

export async function saveAgentRuntimeCredential(input: {
  runtime: AgentRuntimeId;
  apiKey: string;
  apiBase?: string;
}): Promise<AgentRuntime[]> {
  const res = await req(`/api/agent-runtimes/${encodeURIComponent(input.runtime)}/credentials`, {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ api_key: input.apiKey, api_base: input.apiBase }),
  });
  const data = await jsonOrThrow<{ runtimes: AgentRuntime[] }>(res);
  return data.runtimes;
}

export async function deleteAgentRuntimeCredential(runtime: AgentRuntimeId): Promise<void> {
  await jsonOrThrow(
    await req(`/api/agent-runtimes/${encodeURIComponent(runtime)}/credentials`, {
      method: "DELETE",
    }),
  );
}

export async function listAgents(): Promise<Agent[]> {
  const res = await req("/api/agents");
  const data = await jsonOrThrow<{ agents: Agent[] }>(res);
  return data.agents;
}

export interface AvailableProvider {
  id: string;
  name: string;
  description: string;
  default_base_url: string;
}

export interface ConnectedProvider {
  id: string;
  name: string;
  api_base: string;
  masked_api_key: string;
}

export interface ProvidersResponse {
  available_providers: AvailableProvider[];
  connected_providers: ConnectedProvider[];
}

export async function listProviders(): Promise<ProvidersResponse> {
  const res = await req("/api/providers");
  return jsonOrThrow<ProvidersResponse>(res);
}

export async function saveProvider(input: {
  providerId: string;
  apiKey: string;
  apiBase: string;
}): Promise<ProvidersResponse> {
  const res = await req(`/api/providers/${encodeURIComponent(input.providerId)}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      api_key: input.apiKey,
      api_base: input.apiBase,
    }),
  });
  return jsonOrThrow<ProvidersResponse>(res);
}

export async function deleteProvider(providerId: string): Promise<void> {
  const res = await req(`/api/providers/${encodeURIComponent(providerId)}`, {
    method: "DELETE",
  });
  await jsonOrThrow(res);
}

export async function deleteSession(id: string): Promise<void> {
  try {
    await reqHarness(`/session/${encodeURIComponent(id)}`, { method: "DELETE" });
  } catch {
    /* swallow */
  }
}

export interface LiteLLMHealth {
  ok: boolean;
  modelCount?: number;
  status?: number;
  error?: string;
  base?: string;
  modelsUrl?: string;
}

export async function testLiteLLMConnection(): Promise<LiteLLMHealth> {
  const res = await req("/_litellm/health");
  return jsonOrThrow<LiteLLMHealth>(res);
}

export interface HarnessServerHealth {
  ok: boolean;
  mode: "local" | "remote";
  base?: string;
  status?: number;
  error?: string;
}

export async function testHarnessServer(
  rawUrl?: string,
  rawKey?: string,
): Promise<HarnessServerHealth> {
  const base = normalizeHarnessServerUrl(rawUrl ?? getHarnessServerUrl());
  if (!base) return { ok: true, mode: "local" };

  try {
    const res = await fetch(
      harnessProxyPath("/session", base),
      withHarnessProxyAuth(undefined, rawKey ?? getHarnessServerKey()),
    );
    if (!res.ok) {
      const body = await res.text().catch(() => "");
      return {
        ok: false,
        mode: "remote",
        base,
        status: res.status,
        error: body || `HTTP ${res.status}`,
      };
    }
    return { ok: true, mode: "remote", base, status: res.status };
  } catch (err) {
    return {
      ok: false,
      mode: "remote",
      base,
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

export interface GatewayApiKey {
  id: string;
  label?: string | null;
  created_at: number;
  last_used_at?: number | null;
}

export interface CreatedGatewayApiKey extends GatewayApiKey {
  key: string;
}

export async function listGatewayApiKeys(): Promise<GatewayApiKey[]> {
  const res = await req("/api/keys");
  const data = await jsonOrThrow<{ keys: GatewayApiKey[] }>(res);
  return data.keys;
}

export async function createGatewayApiKey(label?: string): Promise<CreatedGatewayApiKey> {
  const res = await req("/api/keys", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ label }),
  });
  return jsonOrThrow<CreatedGatewayApiKey>(res);
}

export async function deleteGatewayApiKey(id: string): Promise<void> {
  const res = await req(`/api/keys/${encodeURIComponent(id)}`, { method: "DELETE" });
  if (!res.ok && res.status !== 404) {
    const body = await res.text().catch(() => "");
    throw new ApiError(res.status, body);
  }
}

export async function getSession(id: string): Promise<OpencodeSession> {
  const res = await reqHarness(`/session/${encodeURIComponent(id)}`);
  return jsonOrThrow<OpencodeSession>(res);
}

export async function getMessages(sid: string): Promise<HarnessMessage[]> {
  const res = await reqHarness(`/session/${encodeURIComponent(sid)}/message`);
  return jsonOrThrow<HarnessMessage[]>(res);
}

export async function sendMessage(opts: {
  sessionId: string;
  text: string;
  model: string;
}): Promise<void> {
  const res = await reqHarness(
    `/session/${encodeURIComponent(opts.sessionId)}/prompt_async`,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        model: { providerID: "litellm", modelID: opts.model },
        parts: [{ type: "text", text: opts.text }],
      }),
    },
  );
  if (res.status === 204) return;
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new ApiError(res.status, body);
  }
}

export async function sendMessageWithRuntimeModel(opts: {
  sessionId: string;
  text: string;
  model: string;
  runtime?: AgentRuntimeId;
}): Promise<void> {
  const model =
    opts.runtime === "claude_managed_agents"
      ? "anthropic/*"
      : opts.runtime === "cursor"
        ? "cursor/*"
        : opts.model;
  return sendMessage({ sessionId: opts.sessionId, text: opts.text, model });
}

export async function abortSession(id: string): Promise<void> {
  await reqHarness(`/session/${encodeURIComponent(id)}/abort`, { method: "POST" });
}

export async function listModels(): Promise<string[]> {
  const res = await req("/v1/models");
  if (!res.ok) return [];
  const data = await res.json().catch(() => null);
  const items: Array<{ id: string }> = data?.data ?? [];
  return items.map((m) => m.id).filter(Boolean);
}

export async function listSpendLogs(input?: {
  q?: string;
  status?: string;
  model?: string;
  limit?: number;
  offset?: number;
}): Promise<SpendLog[]> {
  const params = new URLSearchParams();
  if (input?.q) params.set("q", input.q);
  if (input?.status) params.set("status", input.status);
  if (input?.model) params.set("model", input.model);
  if (input?.limit) params.set("limit", String(input.limit));
  if (input?.offset) params.set("offset", String(input.offset));
  const qs = params.toString();
  const res = await req(`/api/observability/logs${qs ? `?${qs}` : ""}`);
  const data = await jsonOrThrow<{ logs: SpendLog[] }>(res);
  return data.logs ?? [];
}

export async function getSpendLog(requestId: string): Promise<SpendLog> {
  const res = await req(`/api/observability/logs/${encodeURIComponent(requestId)}`);
  return jsonOrThrow<SpendLog>(res);
}

export interface PendingApproval {
  id: string;
  tool: string;
  arguments: Record<string, unknown>;
  createdAt: number;
}

export async function listApprovals(): Promise<PendingApproval[]> {
  const res = await req("/api/approvals");
  const data = await jsonOrThrow<{ approvals: PendingApproval[] }>(res);
  return data.approvals ?? [];
}

export async function acceptApproval(
  id: string,
  args?: Record<string, unknown>,
): Promise<void> {
  const res = await req(`/api/approvals/${encodeURIComponent(id)}/accept`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(args ? { arguments: args } : {}),
  });
  await jsonOrThrow(res);
}

export async function rejectApproval(id: string, feedback?: string): Promise<void> {
  const res = await req(`/api/approvals/${encodeURIComponent(id)}/reject`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(feedback ? { feedback } : {}),
  });
  await jsonOrThrow(res);
}

// ── Agent inbox (/api/inbox) ────────────────────────────────────────────────
// Unified list of human-in-the-loop approvals (kind="approval") an agent is
// blocked on, plus informational issues an agent filed (kind="issue").

export type InboxKind = "approval" | "issue";
export type InboxStatus = "pending" | "accepted" | "rejected" | "open" | "resolved";
export type InboxFilter = "attention" | "completed" | "all";

export interface InboxItem {
  id: string;
  kind: InboxKind;
  title: string;
  sessionId: string | null;
  agent: string | null;
  body: string | null;
  /** Approval tool arguments (editable fields) — present for kind="approval". */
  args?: Record<string, unknown>;
  status: InboxStatus;
  feedback: string | null;
  createdAt: number;
  resolvedAt: number | null;
}

export async function listInbox(filter: InboxFilter = "all"): Promise<InboxItem[]> {
  const res = await req(`/api/inbox?filter=${encodeURIComponent(filter)}`);
  const data = await jsonOrThrow<{ items: InboxItem[] }>(res);
  return data.items ?? [];
}

/** Mark an inbox issue done. */
export async function resolveInboxItem(id: string, note?: string): Promise<void> {
  const res = await req(`/api/inbox/${encodeURIComponent(id)}/resolve`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(note ? { note } : {}),
  });
  await jsonOrThrow(res);
}

// ── Integrations / vault ──────────────────────────────────────────────────────
// API keys are stored in the harness's encrypted vault via /api/vault/:userId.
// When the backend vault is unreachable (e.g. running the UI standalone via
// `next dev`), we transparently fall back to sessionStorage so the flow still
// works. Per project policy, secrets only ever touch sessionStorage — never
// localStorage.

const VAULT_USER = "default";
const VAULT_FALLBACK_PREFIX = "lite-harness-integration:";

function fallbackSet(key: string, value: string): void {
  if (typeof window === "undefined") return;
  try {
    window.sessionStorage.setItem(VAULT_FALLBACK_PREFIX + key, value);
  } catch {
    /* noop */
  }
}

function fallbackDelete(key: string): void {
  if (typeof window === "undefined") return;
  try {
    window.sessionStorage.removeItem(VAULT_FALLBACK_PREFIX + key);
  } catch {
    /* noop */
  }
}

function fallbackList(): string[] {
  if (typeof window === "undefined") return [];
  const keys: string[] = [];
  try {
    for (let i = 0; i < window.sessionStorage.length; i++) {
      const k = window.sessionStorage.key(i);
      if (k?.startsWith(VAULT_FALLBACK_PREFIX)) {
        keys.push(k.slice(VAULT_FALLBACK_PREFIX.length));
      }
    }
  } catch {
    /* noop */
  }
  return keys;
}

/** Store an integration's API key. Returns the storage backend that took it. */
export async function saveIntegrationKey(
  envKey: string,
  value: string,
): Promise<"vault" | "session"> {
  try {
    const res = await req(`/api/vault/${VAULT_USER}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ key: envKey, value }),
    });
    if (res.ok) return "vault";
  } catch {
    /* fall through to sessionStorage */
  }
  fallbackSet(envKey, value);
  return "session";
}

/** Remove a stored integration key from both vault and sessionStorage. */
export async function deleteIntegrationKey(envKey: string): Promise<void> {
  try {
    await req(`/api/vault/${VAULT_USER}/${encodeURIComponent(envKey)}`, {
      method: "DELETE",
    });
  } catch {
    /* noop */
  }
  fallbackDelete(envKey);
}

/** List the env-key names that currently have a stored value. */
export async function listIntegrationKeys(): Promise<string[]> {
  const keys = new Set<string>(fallbackList());
  try {
    const res = await req(`/api/vault/${VAULT_USER}`);
    if (res.ok) {
      const data = (await res.json()) as { keys?: { key: string }[] };
      for (const k of data.keys ?? []) keys.add(k.key);
    }
  } catch {
    /* vault unavailable — sessionStorage only */
  }
  return [...keys];
}

export interface VaultKeyEntry {
  key: string;
  updated_at?: number;
  source?: string;
}

/** List all vault keys with metadata (no values). */
export async function listVaultKeys(): Promise<VaultKeyEntry[]> {
  const fallback: VaultKeyEntry[] = fallbackList().map((k) => ({ key: k }));
  const byKey = new Map<string, VaultKeyEntry>(fallback.map((e) => [e.key, e]));
  try {
    const res = await req(`/api/vault/${VAULT_USER}`);
    if (res.ok) {
      const data = (await res.json()) as { keys?: VaultKeyEntry[] };
      for (const k of data.keys ?? []) byKey.set(k.key, k);
    }
  } catch {
    /* vault unavailable — sessionStorage only */
  }
  return [...byKey.values()];
}

// ── Skills CRUD (DB-backed, /api/skills) ──────────────────────────────────────
// Skills are reusable capability docs persisted in the harness DB and attached
// to agents via agents.skill_ids.

export async function createSkill(input: {
  name: string;
  content: string;
  description?: string | null;
}): Promise<Skill> {
  const res = await req("/api/skills", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(input),
  });
  return jsonOrThrow<Skill>(res);
}

export async function getSkill(id: string): Promise<Skill> {
  const res = await req(`/api/skills/${encodeURIComponent(id)}`);
  return jsonOrThrow<Skill>(res);
}

export async function updateSkill(
  id: string,
  fields: { name?: string; description?: string | null; content?: string },
): Promise<Skill> {
  const res = await req(`/api/skills/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(fields),
  });
  return jsonOrThrow<Skill>(res);
}

export async function deleteSkill(id: string): Promise<void> {
  await req(`/api/skills/${encodeURIComponent(id)}`, { method: "DELETE" });
}

/** Attach a skill to an agent (idempotent — no-op if already attached). */
export async function attachSkillToAgent(agentId: string, skillId: string): Promise<void> {
  const res = await req(`/api/agents/${encodeURIComponent(agentId)}`);
  const agent = await jsonOrThrow<Agent>(res);
  const next = Array.from(new Set([...(agent.skill_ids ?? []), skillId]));
  await req(`/api/agents/${encodeURIComponent(agentId)}`, {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ skill_ids: next }),
  });
}

export function subscribeEvents(opts: {
  sessionId: string;
  onEvent: (ev: unknown) => void;
  onError?: (err: unknown) => void;
}): () => void {
  let es: EventSource | null = null;
  try {
    es = new EventSource(harnessEventSourceUrl());
  } catch (e) {
    opts.onError?.(e);
    return () => {};
  }
  es.onmessage = (msg) => {
    try {
      const data = JSON.parse(msg.data);
      const sid =
        (data?.properties?.sessionID as string | undefined) ??
        (data?.properties?.info?.sessionID as string | undefined) ??
        (data?.properties?.part?.sessionID as string | undefined);
      if (sid === opts.sessionId) opts.onEvent(data);
    } catch (e) {
      opts.onError?.(e);
    }
  };
  es.onerror = (e) => opts.onError?.(e);
  return () => {
    try {
      es?.close();
    } catch {
      /* noop */
    }
  };
}

export interface RuntimeAgentEvent {
  type: string;
  [key: string]: unknown;
}

export function subscribeRuntimeEvents(opts: {
  sessionId: string;
  onEvent: (ev: RuntimeAgentEvent) => void;
  onError?: (err: unknown) => void;
}): () => void {
  let es: EventSource | null = null;
  try {
    es = new EventSource(runtimeEventSourceUrl(opts.sessionId));
  } catch (e) {
    opts.onError?.(e);
    return () => {};
  }
  es.onmessage = (msg) => {
    try {
      opts.onEvent(JSON.parse(msg.data) as RuntimeAgentEvent);
    } catch (e) {
      opts.onError?.(e);
    }
  };
  es.onerror = (e) => opts.onError?.(e);
  return () => {
    try {
      es?.close();
    } catch {
      /* noop */
    }
  };
}

export function runtimeEventSourceUrl(sessionId: string): string {
  const localKey = getStoredMasterKey();
  const qs = localKey ? `?key=${encodeURIComponent(localKey)}` : "";
  return `${BASE}/session/${encodeURIComponent(sessionId)}/runtime_events${qs}`;
}

export function harnessEventSourceUrl(): string {
  const remoteBase = getHarnessServerUrl();
  const localKey = getStoredMasterKey();
  if (!remoteBase) {
    const qs = localKey ? `?key=${encodeURIComponent(localKey)}` : "";
    return `${BASE}/event${qs}`;
  }

  const qs = new URLSearchParams({ base: remoteBase });
  if (localKey) qs.set("key", localKey);
  const targetKey = getHarnessServerKey();
  if (targetKey) qs.set("target_key", targetKey);
  return `${BASE}/api/harness-proxy/event?${qs.toString()}`;
}

// ── Agent CRUD (/api/agents) ────────────────────────────────────────────────
export async function createAgent(
  input: { name: string; owner_id: string; schedule?: { cron: string; timezone?: string } | null } & Partial<Agent>,
): Promise<Agent> {
  const res = await req("/api/agents", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(input),
  });
  return jsonOrThrow<Agent>(res);
}

export async function getAgent(id: string): Promise<Agent> {
  const res = await req(`/api/agents/${encodeURIComponent(id)}`);
  return jsonOrThrow<Agent>(res);
}

export async function runAgent(agentId: string, prompt: string): Promise<AgentRunStart> {
  const res = await req(`/api/agents/${encodeURIComponent(agentId)}/run`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ prompt }),
  });
  return jsonOrThrow<AgentRunStart>(res);
}

export async function updateAgent(id: string, fields: Partial<Agent>): Promise<Agent> {
  const res = await req(`/api/agents/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(fields),
  });
  return jsonOrThrow<Agent>(res);
}

export async function deleteAgent(id: string): Promise<void> {
  await req(`/api/agents/${encodeURIComponent(id)}`, { method: "DELETE" });
}

export async function listAgentFiles(agentId: string): Promise<AgentFile[]> {
  const res = await req(`/api/agents/${encodeURIComponent(agentId)}/files`);
  const data = await jsonOrThrow<{ files: AgentFile[] }>(res);
  return data.files ?? [];
}

export async function downloadAgentFile(agentId: string, filePath: string): Promise<Blob> {
  const res = await req(
    `/api/agents/${encodeURIComponent(agentId)}/files/${encodeURIComponent(filePath)}`,
  );
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new ApiError(res.status, body);
  }
  return res.blob();
}

// ── Skills list (DB-backed, /api/skills) ──────────────────────────────────────
export async function listSkills(): Promise<Skill[]> {
  const res = await req("/api/skills");
  const data = await jsonOrThrow<{ skills: Skill[] }>(res);
  return data.skills ?? [];
}

// ── Agent memory (/api/agents/:id/memory) ─────────────────────────────────────
// The same per-agent key→value notes the agent reads & writes via its memory_*
// tools. Surfaced here so the UI can show and curate what an agent remembers.
export async function listMemory(agentId: string): Promise<Memory[]> {
  const res = await req(`/api/agents/${encodeURIComponent(agentId)}/memory`);
  const data = await jsonOrThrow<{ memories: Memory[] }>(res);
  return data.memories ?? [];
}

export async function storeMemory(
  agentId: string,
  key: string,
  value: string,
  alwaysOn?: boolean,
): Promise<Memory> {
  const res = await req(`/api/agents/${encodeURIComponent(agentId)}/memory`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ key, value, ...(typeof alwaysOn === "boolean" ? { always_on: alwaysOn } : {}) }),
  });
  return jsonOrThrow<Memory>(res);
}

export async function deleteMemory(agentId: string, key: string): Promise<void> {
  await req(
    `/api/agents/${encodeURIComponent(agentId)}/memory/${encodeURIComponent(key)}`,
    { method: "DELETE" },
  );
}
