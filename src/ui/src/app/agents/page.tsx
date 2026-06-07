"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Clock, Plus, Play, Pencil, Trash2, X, Brain, Plug } from "lucide-react";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { BrandIcon } from "@/components/brand-icons";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { ScheduleEditor } from "@/components/schedule-editor";
import {
  listAgents,
  updateAgent,
  deleteAgent,
  listSkills,
  listVaultKeys,
  listPlatformMcps,
  saveIntegrationKey,
  deleteIntegrationKey,
  listMemory,
  storeMemory,
  deleteMemory,
} from "@/lib/api";
import { DEFAULT_TIMEZONE, scheduleLabel } from "@/lib/schedule";
import type { Agent, Skill, Memory, VaultKeyEntry, PlatformMcp } from "@/lib/types";
import {
  slackActionClass,
  slackActionLabel,
  slackConfig,
  useSlackAppFlow,
} from "./slack-app-flow";

interface FormState {
  name: string;
  description: string;
  prompt: string;
  skill_ids: string[];
  cron: string;
  timezone: string;
  vault_keys: string[];
  platform_mcp_ids: string[];
}

const EMPTY: FormState = {
  name: "",
  description: "",
  prompt: "",
  skill_ids: [],
  cron: "",
  timezone: DEFAULT_TIMEZONE,
  vault_keys: [],
  platform_mcp_ids: [],
};

function agentConfig(agent: Agent): Record<string, unknown> {
  return agent.config && typeof agent.config === "object" && !Array.isArray(agent.config)
    ? (agent.config as Record<string, unknown>)
    : {};
}

function platformMcpIds(agent: Agent): string[] {
  const config = agentConfig(agent);
  const value = config.platform_mcp_ids ?? config.platformMcpIds;
  return Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : [];
}

export default function AgentsPage() {
  const router = useRouter();
  const [agents, setAgents] = useState<Agent[] | null>(null);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [platformMcps, setPlatformMcps] = useState<PlatformMcp[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState<FormState>(EMPTY);
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [vaultKeyInput, setVaultKeyInput] = useState("");
  const [vaultValues, setVaultValues] = useState<Record<string, string>>({});
  const [storedKeyEntries, setStoredKeyEntries] = useState<VaultKeyEntry[]>([]);
  const [memories, setMemories] = useState<Memory[] | null>(null);
  const [memKey, setMemKey] = useState("");
  const [memValue, setMemValue] = useState("");
  const slackFlow = useSlackAppFlow(setAgents);

  const load = async () => {
    try {
      setAgents((await listAgents()) as Agent[]);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  useEffect(() => {
    load();
    listSkills().then(setSkills).catch(() => setSkills([]));
    listPlatformMcps().then(setPlatformMcps).catch(() => setPlatformMcps([]));
    listVaultKeys().then(setStoredKeyEntries).catch(() => setStoredKeyEntries([]));
  }, []);

  const addVaultKey = () => {
    const k = vaultKeyInput.trim();
    if (!k) return;
    setForm((f) => (f.vault_keys.includes(k) ? f : { ...f, vault_keys: [...f.vault_keys, k] }));
    setVaultKeyInput("");
  };
  const removeVaultKey = (k: string) => {
    setForm((f) => ({ ...f, vault_keys: f.vault_keys.filter((x) => x !== k) }));
    deleteIntegrationKey(k).then(() =>
      setStoredKeyEntries((p) => p.filter((x) => x.key !== k))
    ).catch(() => {});
    setVaultValues(({ [k]: _drop, ...rest }) => rest);
  };
  const saveVaultValue = async (k: string) => {
    const v = vaultValues[k];
    if (!v) return;
    try {
      await saveIntegrationKey(k, v, "personal");
      setStoredKeyEntries((p) =>
        p.some((x) => x.key === k)
          ? p
          : [...p, { key: k, scope: "personal" }]
      );
      setVaultValues(({ [k]: _drop, ...rest }) => rest);
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    }
  };

  const toggleSkill = (id: string) =>
    setForm((f) => ({
      ...f,
      skill_ids: f.skill_ids.includes(id)
        ? f.skill_ids.filter((s) => s !== id)
        : [...f.skill_ids, id],
    }));

  const togglePlatformMcp = (id: string) =>
    setForm((f) => ({
      ...f,
      platform_mcp_ids: f.platform_mcp_ids.includes(id)
        ? f.platform_mcp_ids.filter((mcpId) => mcpId !== id)
        : [...f.platform_mcp_ids, id],
    }));

  const skillName = (id: string) => skills.find((s) => s.id === id)?.name ?? id;
  const platformMcpName = (id: string) =>
    platformMcps.find((mcp) => mcp.id === id)?.name ?? id;

  const loadMemory = async (agentId: string) => {
    setMemories(null);
    try {
      setMemories(await listMemory(agentId));
    } catch {
      setMemories([]);
    }
  };
  const addMemory = async () => {
    const k = memKey.trim();
    if (!editingId || !k || !memValue.trim()) return;
    try {
      await storeMemory(editingId, k, memValue);
      setMemKey("");
      setMemValue("");
      await loadMemory(editingId);
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    }
  };
  const removeMemory = async (key: string) => {
    if (!editingId) return;
    setMemories((prev) => prev?.filter((m) => m.key !== key) ?? null);
    try {
      await deleteMemory(editingId, key);
    } catch {
      loadMemory(editingId);
    }
  };

  const openEdit = (ag: Agent) => {
    setEditingId(ag.id);
    setForm({
      name: ag.name ?? "",
      description: ag.description ?? "",
      prompt: ag.prompt ?? "",
      skill_ids: Array.isArray(ag.skill_ids) ? ag.skill_ids : [],
      cron: ag.cron ?? "",
      timezone: ag.timezone ?? DEFAULT_TIMEZONE,
      vault_keys: Array.isArray(ag.vault_keys) ? ag.vault_keys : [],
      platform_mcp_ids: platformMcpIds(ag),
    });
    setFormError(null);
    setVaultKeyInput("");
    setVaultValues({});
    setMemKey("");
    setMemValue("");
    loadMemory(ag.id);
    setOpen(true);
  };

  const save = async () => {
    setSaving(true);
    setFormError(null);
    try {
      if (!form.name.trim()) throw new Error("Name is required");
      if (!editingId) throw new Error("Agent ID is required");
      const cron = form.cron.trim();
      const timezone = form.timezone.trim() || "UTC";
      const currentAgent = agents?.find((agent) => agent.id === editingId);
      const config = {
        ...(currentAgent ? agentConfig(currentAgent) : {}),
        platform_mcp_ids: form.platform_mcp_ids,
      };
      await updateAgent(editingId, {
        name: form.name,
        description: form.description,
        prompt: form.prompt,
        skill_ids: form.skill_ids,
        cron: cron || null,
        timezone,
        vault_keys: form.vault_keys,
        config,
      });
      setOpen(false);
      await load();
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const remove = async (ag: Agent) => {
    if (!confirm(`Delete agent "${String(ag.name)}"?`)) return;
    setAgents((prev) => prev?.filter((x) => x.id !== ag.id) ?? null);
    try {
      await deleteAgent(ag.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      load();
    }
  };

  const openAgent = (ag: Agent) => {
    router.push(`/sessions/?agent=${encodeURIComponent(ag.id)}`);
  };

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar />
      <div className="flex-1 flex flex-col min-w-0">
        <header className="h-12 border-b border-border flex items-center justify-between px-4 shrink-0">
          <h1 className="text-sm font-semibold">Agents</h1>
          <div className="flex items-center gap-2">
            <Button size="sm" onClick={() => router.push("/agents/new/")}>
              <Plus className="size-4" />
              Create agent
            </Button>
            <ThemeToggle />
          </div>
        </header>

        <main className="flex-1 overflow-y-auto">
          <div className="max-w-4xl mx-auto px-4 py-6 flex flex-col gap-3">
            {error && (
              <Card className="border-destructive p-3">
                <p className="text-sm text-destructive">{error}</p>
              </Card>
            )}
            {!agents && !error && (
              <div className="text-sm text-muted-foreground">Loading…</div>
            )}
            {agents && agents.length === 0 && (
              <div className="text-center text-sm text-muted-foreground py-16">
                No agents yet. Start with a template or draft one from a prompt.
              </div>
            )}
            {agents?.map((ag) => {
              const slack = slackConfig(ag);
              const attachedPlatformMcps = platformMcpIds(ag);
              return (
                <Card
                  key={String(ag.id)}
                  className="p-4 flex items-start justify-between gap-4 cursor-pointer hover:bg-muted/40 transition-colors"
                  onClick={() => router.push(`/agents/detail/?id=${encodeURIComponent(ag.id)}`)}
                >
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-sm truncate">{String(ag.name)}</span>
                    {Boolean(ag.model) && (
                      <span className="font-mono text-[10px] bg-muted text-muted-foreground rounded px-1.5 py-0.5">{String(ag.model)}</span>
                    )}
                  </div>
                  {Boolean(ag.description) && (
                    <p className="text-xs text-muted-foreground mt-1 line-clamp-2">{String(ag.description)}</p>
                  )}
                  {Boolean(ag.prompt) && (
                    <p className="text-xs text-muted-foreground/70 mt-1 line-clamp-1 font-mono">{String(ag.prompt)}</p>
                  )}
                  <p className="text-xs text-muted-foreground mt-1.5 flex items-center gap-1.5">
                    <Clock className="size-3" />
                    <span className="font-mono text-[11px]">{scheduleLabel(ag.cron, ag.timezone)}</span>
                  </p>
                  {Array.isArray(ag.skill_ids) && ag.skill_ids.length > 0 && (
                    <div className="flex flex-wrap gap-1 mt-1.5">
                      {ag.skill_ids.map((id) => (
                        <Badge key={id} variant="secondary" className="text-[10px]">
                          {skillName(id)}
                        </Badge>
                      ))}
                    </div>
                  )}
                  {attachedPlatformMcps.length > 0 && (
                    <div className="flex flex-wrap gap-1 mt-1.5">
                      {attachedPlatformMcps.map((id) => (
                        <Badge key={id} variant="outline" className="text-[10px] gap-1">
                          <Plug className="size-3" />
                          {platformMcpName(id)}
                        </Badge>
                      ))}
                    </div>
                  )}
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  <Button size="sm" variant="default" onClick={(e) => { e.stopPropagation(); openAgent(ag); }}>
                    <Play className="size-3.5" />
                    Run
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    className={slackActionClass(slack)}
                    onClick={(e) => { e.stopPropagation(); slackFlow.openSlack(ag); }}
                    title={
                      slack.status === "connected"
                        ? `${slack.slack_team_name || "Slack"}${slack.bot_user_id ? ` · <@${slack.bot_user_id}>` : ""}`
                        : slack.oauth_error || undefined
                    }
                  >
                    <BrandIcon id="slack" className="size-3.5" />
                    {slackActionLabel(slack)}
                  </Button>
                  <Button size="sm" variant="outline" onClick={(e) => { e.stopPropagation(); openEdit(ag); }} aria-label="Edit">
                    <Pencil className="size-3.5" />
                  </Button>
                  <Button size="sm" variant="outline" onClick={(e) => { e.stopPropagation(); remove(ag); }} aria-label="Delete">
                    <Trash2 className="size-3.5" />
                  </Button>
                </div>
              </Card>
              );
            })}
          </div>
        </main>
      </div>

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="w-[92vw] sm:max-w-2xl max-h-[88vh] grid-rows-[auto_minmax(0,1fr)_auto] gap-0 p-0">
          <DialogHeader className="px-6 pt-6 pb-4 border-b border-border">
            <DialogTitle>Edit agent</DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-4 px-6 py-4 overflow-y-auto">
            <div className="grid gap-1.5">
              <Label htmlFor="ag-name">Name</Label>
              <Input
                id="ag-name"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                placeholder="security-reviewer"
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="ag-desc">Description</Label>
              <Input
                id="ag-desc"
                value={form.description}
                onChange={(e) => setForm({ ...form, description: e.target.value })}
                placeholder="What this agent does"
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="ag-prompt">System prompt</Label>
              <Textarea
                id="ag-prompt"
                value={form.prompt}
                onChange={(e) => setForm({ ...form, prompt: e.target.value })}
                rows={10}
                placeholder="You are a meticulous security reviewer…"
              />
            </div>
            <ScheduleEditor
              cron={form.cron}
              timezone={form.timezone}
              onChange={(next) => setForm({ ...form, ...next })}
            />
            <div className="grid gap-1.5">
              <Label>Skills</Label>
              {skills.length === 0 ? (
                <p className="text-xs text-muted-foreground">
                  No skills available on this server.
                </p>
              ) : (
                <div className="max-h-44 overflow-y-auto rounded-md border border-border divide-y divide-border">
                  {skills.map((s) => {
                    const checked = form.skill_ids.includes(s.id);
                    return (
                      <label
                        key={s.id}
                        className="flex items-start gap-2 px-2.5 py-1.5 cursor-pointer hover:bg-muted/50"
                      >
                        <input
                          type="checkbox"
                          className="mt-0.5"
                          checked={checked}
                          onChange={() => toggleSkill(s.id)}
                        />
                        <span className="min-w-0 flex flex-col">
                          <span className="text-xs font-medium">{s.name}</span>
                          {s.description && (
                            <span className="text-[11px] text-muted-foreground line-clamp-2">
                              {s.description}
                            </span>
                          )}
                        </span>
                      </label>
                    );
                  })}
                </div>
              )}
              {form.skill_ids.length > 0 && (
                <p className="text-[11px] text-muted-foreground">
                  {form.skill_ids.length} skill{form.skill_ids.length === 1 ? "" : "s"} attached
                </p>
              )}
            </div>
            <div className="grid gap-1.5">
              <Label className="flex items-center gap-1.5">
                <Plug className="size-3.5" />
                Platform MCPs
              </Label>
              {platformMcps.length === 0 ? (
                <p className="text-xs text-muted-foreground">
                  No platform MCPs available on this server.
                </p>
              ) : (
                <div className="rounded-md border border-border divide-y divide-border">
                  {platformMcps.map((mcp) => {
                    const checked = form.platform_mcp_ids.includes(mcp.id);
                    return (
                      <label
                        key={mcp.id}
                        className="flex items-start gap-2 px-2.5 py-1.5 cursor-pointer hover:bg-muted/50"
                      >
                        <input
                          type="checkbox"
                          className="mt-0.5"
                          checked={checked}
                          onChange={() => togglePlatformMcp(mcp.id)}
                        />
                        <span className="min-w-0 flex flex-col">
                          <span className="text-xs font-medium">{mcp.name}</span>
                          <span className="text-[11px] text-muted-foreground line-clamp-2">
                            {mcp.description}
                          </span>
                        </span>
                      </label>
                    );
                  })}
                </div>
              )}
              {form.platform_mcp_ids.length > 0 && (
                <p className="text-[11px] text-muted-foreground">
                  {form.platform_mcp_ids.length} platform MCP
                  {form.platform_mcp_ids.length === 1 ? "" : "s"} attached
                </p>
              )}
            </div>
            <div className="grid gap-1.5">
              <Label>Vault credentials</Label>
              <p className="text-[11px] text-muted-foreground -mt-1">
                Secrets this agent can use. Reference them in the prompt as{" "}
                <span className="font-mono">{"{{vault.KEY_NAME}}"}</span>.
              </p>
              <div className="flex gap-2">
                <Input
                  value={vaultKeyInput}
                  onChange={(e) => setVaultKeyInput(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); addVaultKey(); } }}
                  placeholder="BROWSER_USE_API_KEY"
                  className="font-mono text-xs"
                />
                <Button type="button" variant="outline" size="sm" onClick={addVaultKey}>
                  Add
                </Button>
              </div>
              {form.vault_keys.length > 0 && (
                <div className="rounded-md border border-border divide-y divide-border">
                  {form.vault_keys.map((k) => {
                    const entry = storedKeyEntries.find((x) => x.key === k);
                    const isSet = !!entry;
                    const badgeLabel = isSet
                      ? entry.scope === "global"
                        ? "set (global)"
                        : "set (personal)"
                      : "no value";
                    return (
                      <div key={k} className="flex items-center gap-2 px-2.5 py-1.5">
                        <span className="text-xs font-mono min-w-0 flex-1 truncate">{k}</span>
                        <Badge variant={isSet ? "secondary" : "outline"} className="text-[10px]">
                          {badgeLabel}
                        </Badge>
                        <Input
                          type="password"
                          value={vaultValues[k] ?? ""}
                          onChange={(e) => setVaultValues((v) => ({ ...v, [k]: e.target.value }))}
                          placeholder={isSet ? "update value" : "set value"}
                          className="h-7 w-36 text-xs"
                        />
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          className="h-7"
                          disabled={!vaultValues[k]}
                          onClick={() => saveVaultValue(k)}
                        >
                          Save
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          className="h-7 px-2"
                          onClick={() => removeVaultKey(k)}
                          aria-label={`Remove ${k}`}
                        >
                          <X className="size-3.5" />
                        </Button>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
            {editingId && (
              <div className="grid gap-1.5">
                <Label className="flex items-center gap-1.5">
                  <Brain className="size-3.5" />
                  Memory
                </Label>
                <p className="text-[11px] text-muted-foreground -mt-1">
                  Durable notes this agent stores and recalls across sessions and runs
                  via its <span className="font-mono">memory_*</span> tools.
                </p>
                {memories === null ? (
                  <p className="text-xs text-muted-foreground">Loading…</p>
                ) : memories.length === 0 ? (
                  <p className="text-xs text-muted-foreground">
                    Nothing remembered yet. The agent fills this in as it works — or add a note below.
                  </p>
                ) : (
                  <div className="rounded-md border border-border divide-y divide-border max-h-52 overflow-y-auto">
                    {memories.map((m) => (
                      <div key={m.key} className="flex items-start gap-2 px-2.5 py-1.5">
                        <div className="min-w-0 flex-1">
                          <div className="text-xs font-mono font-medium truncate">{m.key}</div>
                          <div className="text-[11px] text-muted-foreground whitespace-pre-wrap break-words">{m.value}</div>
                        </div>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          className="h-7 px-2 shrink-0"
                          onClick={() => removeMemory(m.key)}
                          aria-label={`Forget ${m.key}`}
                        >
                          <X className="size-3.5" />
                        </Button>
                      </div>
                    ))}
                  </div>
                )}
                <div className="flex gap-2 items-start">
                  <Input
                    value={memKey}
                    onChange={(e) => setMemKey(e.target.value)}
                    placeholder="key"
                    className="font-mono text-xs w-32 shrink-0"
                  />
                  <Textarea
                    value={memValue}
                    onChange={(e) => setMemValue(e.target.value)}
                    placeholder="value to remember"
                    rows={1}
                    className="text-xs"
                  />
                  <Button type="button" variant="outline" size="sm" onClick={addMemory} disabled={!memKey.trim() || !memValue.trim()}>
                    Add
                  </Button>
                </div>
              </div>
            )}
            {formError && <p className="text-sm text-destructive">{formError}</p>}
          </div>
          <DialogFooter className="m-0 rounded-b-xl px-6 py-4">
            <Button variant="outline" onClick={() => setOpen(false)} disabled={saving}>
              Cancel
            </Button>
            <Button onClick={save} disabled={saving}>
              {saving ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {slackFlow.dialog}
    </div>
  );
}
