"use client";

import { useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import {
  ArrowUp,
  Bell,
  Bot,
  CheckCircle2,
  Clipboard,
  Code2,
  Database,
  FileSearch,
  FileText,
  LifeBuoy,
  Loader2,
  Mail,
  Search,
  ShieldCheck,
  Sparkles,
  XCircle,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  AGENT_TEMPLATES,
  agentTemplateForPrompt,
  buildAgentDraftFromPrompt,
  createInputFromDraft,
  parseAgentDraftConfig,
  stringifyAgentDraft,
  withRuntimeDefaultTools,
} from "@/lib/agent-builder";
import type { AgentDraft, AgentTemplate } from "@/lib/agent-builder";
import { apiErrorMessage, createAgent, draftAgentConfigWithModel, listAgentRuntimes } from "@/lib/api";
import { scheduleLabel } from "@/lib/schedule";
import type { AgentRuntime } from "@/lib/types";
import { cn } from "@/lib/utils";

type BuilderStep = "create" | "config";
type BuilderView = "config" | "preview";

const TEMPLATE_ICONS: Record<string, LucideIcon> = {
  blank: Bot,
  "deep-researcher": Search,
  "inbox-triage": Mail,
  "security-reviewer": ShieldCheck,
  "support-agent": LifeBuoy,
  "incident-commander": Bell,
  "data-analyst": Database,
  "sprint-retro": FileText,
};

const INITIAL_CONFIG = stringifyAgentDraft(AGENT_TEMPLATES[0].draft);

export default function NewAgentPage() {
  const router = useRouter();
  const [step, setStep] = useState<BuilderStep>("create");
  const [prompt, setPrompt] = useState("");
  const [selectedTemplateId, setSelectedTemplateId] = useState("blank");
  const [configText, setConfigText] = useState(INITIAL_CONFIG);
  const [runtimes, setRuntimes] = useState<AgentRuntime[]>([]);
  const [view, setView] = useState<BuilderView>("config");
  const [drafting, setDrafting] = useState(false);
  const [lastRequest, setLastRequest] = useState("");
  const [draftNotice, setDraftNotice] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const parsed = useMemo(() => parseAgentDraftConfig(configText), [configText]);
  const draft = parsed.draft;
  const canCreate = !saving && !parsed.error && draft.name.trim().length > 0;

  useEffect(() => {
    listAgentRuntimes()
      .then((values) => {
        setRuntimes(values);
        setConfigText((current) =>
          current === INITIAL_CONFIG
            ? stringifyAgentDraft(withRuntimeDefaultTools(AGENT_TEMPLATES[0].draft, values))
            : current,
        );
      })
      .catch(() => setRuntimes([]));
  }, []);

  const openConfig = (
    next: AgentDraft,
    templateId: string,
    options?: { request?: string; notice?: string | null },
  ) => {
    setSelectedTemplateId(templateId);
    setConfigText(stringifyAgentDraft(next));
    setLastRequest(options?.request ?? next.name);
    setDraftNotice(options?.notice ?? null);
    setView("config");
    setStep("config");
    setError(null);
  };

  const draftFromPrompt = async () => {
    const trimmed = prompt.trim();
    if (!trimmed || drafting) return;
    const templateId = agentTemplateForPrompt(trimmed).id;
    setDrafting(true);
    setError(null);
    setDraftNotice(null);
    setLastRequest(trimmed);
    try {
      const generated = await draftAgentConfigWithModel(trimmed, runtimes);
      const generatedDraft = parseAgentDraftConfig(generated);
      if (generatedDraft.error) throw new Error(generatedDraft.error);
      openConfig(generatedDraft.draft, templateId, { request: trimmed });
    } catch (err) {
      const isServiceError =
        err instanceof Error &&
        (err.message.startsWith("HTTP ") || err.name === "TypeError" || err.name === "AbortError");
      const serviceError = apiErrorMessage(err, "Model drafting failed");
      openConfig(withRuntimeDefaultTools(buildAgentDraftFromPrompt(trimmed), runtimes), templateId, {
        request: trimmed,
        notice: isServiceError
          ? `Model drafting failed: ${serviceError}. Using a local starter config instead.`
          : "Model couldn't generate a valid config for this request, so a local starter config was generated.",
      });
    } finally {
      setDrafting(false);
    }
  };

  const create = async () => {
    const current = parseAgentDraftConfig(configText);
    if (current.error) {
      setError(current.error);
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const agent = await createAgent(createInputFromDraft(current.draft));
      router.push(`/agents/detail/?id=${encodeURIComponent(agent.id)}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create agent");
    } finally {
      setSaving(false);
    }
  };

  const copyConfig = async () => {
    try {
      await navigator.clipboard.writeText(configText);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1300);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
          <div className="flex min-w-0 items-center gap-2">
            <Button
              size="sm"
              variant="ghost"
              onClick={() => router.push("/agents/")}
              className="gap-1.5 text-muted-foreground hover:text-foreground"
            >
              Agents
            </Button>
            <span className="text-muted-foreground">/</span>
            <span className="truncate text-sm font-semibold">Create agent</span>
          </div>
          <div className="flex items-center gap-2">
            {step === "config" && (
              <Button size="sm" onClick={() => void create()} disabled={!canCreate}>
                <CheckCircle2 className="size-3.5" />
                {saving ? "Creating..." : "Create agent"}
              </Button>
            )}
            <Button
              size="sm"
              variant="outline"
              onClick={() => router.push("/agents/")}
              className="hidden sm:inline-flex"
            >
              Cancel
            </Button>
            <ThemeToggle />
          </div>
        </header>

        <main className="min-h-0 flex-1 overflow-y-auto bg-[#fbfbfa] text-[#20201f] dark:bg-background dark:text-foreground">
          <PlatformSteps activeStep={step === "create" ? 1 : 2} />
          {step === "create" ? (
            <CreateStep
              draft={draft}
              drafting={drafting}
              prompt={prompt}
              selectedTemplateId={selectedTemplateId}
              onPromptChange={setPrompt}
              onGenerate={draftFromPrompt}
              onTemplateSelect={(template) =>
                openConfig(withRuntimeDefaultTools(template.draft, runtimes), template.id, { request: template.title })
              }
            />
          ) : (
            <ConfigStep
              canCreate={canCreate}
              configText={configText}
              copied={copied}
              draft={draft}
              draftNotice={draftNotice}
              drafting={drafting}
              error={error}
              lastRequest={lastRequest}
              parsedError={parsed.error}
              prompt={prompt}
              saving={saving}
              view={view}
              onConfigChange={(next) => {
                setConfigText(next);
                setError(null);
              }}
              onCopy={() => void copyConfig()}
              onCreate={() => void create()}
              onPromptChange={setPrompt}
              onRefine={draftFromPrompt}
              onViewChange={setView}
            />
          )}
        </main>
      </div>
    </div>
  );
}

function PlatformSteps({ activeStep }: { activeStep: 1 | 2 }) {
  return (
    <div className="border-b border-border bg-background/80 px-4 py-3 backdrop-blur">
      <div className="mx-auto flex max-w-7xl items-center gap-3">
        <StepMarker active={activeStep === 1} index={1} label="Create agent" suffix="POST /v1/agents" />
        <div className="h-px w-10 bg-border" />
        <StepMarker active={activeStep === 2} index={2} label="Edit config" />
      </div>
    </div>
  );
}

function StepMarker({
  active,
  index,
  label,
  suffix,
}: {
  active: boolean;
  index: number;
  label: string;
  suffix?: string;
}) {
  return (
    <div className={cn("flex min-w-0 items-center gap-2", active ? "text-foreground" : "text-muted-foreground")}>
      <span
        className={cn(
          "flex size-6 shrink-0 items-center justify-center rounded-full text-xs font-semibold",
          active ? "bg-foreground text-background" : "bg-muted text-muted-foreground",
        )}
      >
        {index}
      </span>
      <span className="truncate text-sm font-semibold">{label}</span>
      {suffix && <span className="hidden font-mono text-xs text-muted-foreground sm:inline">{suffix}</span>}
    </div>
  );
}

function CreateStep({
  draft,
  drafting,
  prompt,
  selectedTemplateId,
  onPromptChange,
  onGenerate,
  onTemplateSelect,
}: {
  draft: AgentDraft;
  drafting: boolean;
  prompt: string;
  selectedTemplateId: string;
  onPromptChange: (next: string) => void;
  onGenerate: () => void;
  onTemplateSelect: (template: AgentTemplate) => void;
}) {
  return (
    <div className="grid min-h-[calc(100vh-6.5rem)] gap-6 px-4 py-6 lg:grid-cols-[minmax(420px,1fr)_minmax(520px,0.98fr)]">
      <section className="relative flex min-h-[560px] flex-col rounded-lg border border-transparent px-2 pb-2 sm:px-4">
        <div className="flex flex-1 items-center justify-center pb-24 text-center">
          {drafting ? (
            <div className="grid w-full max-w-2xl justify-items-center gap-5">
              <div className="ml-auto max-w-[82%] rounded-lg bg-foreground px-4 py-3 text-left text-sm text-background">
                {prompt.trim()}
              </div>
              <div className="flex items-center gap-2 text-sm font-medium text-muted-foreground">
                <Loader2 className="size-4 animate-spin text-foreground" />
                Drafting config.yaml
              </div>
            </div>
          ) : (
            <div>
              <h1 className="text-2xl font-semibold text-[#20201f] dark:text-foreground">
                What do you want to build?
              </h1>
              <p className="mt-4 text-base text-muted-foreground">
                Describe your agent or start with a template.
              </p>
            </div>
          )}
        </div>

        <div className="mx-auto w-full max-w-3xl overflow-hidden rounded-lg border border-border bg-card shadow-[0_18px_70px_rgba(15,23,42,0.10)]">
          <Textarea
            value={prompt}
            onChange={(event) => onPromptChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey && !drafting) {
                event.preventDefault();
                onGenerate();
              }
            }}
            placeholder="Describe your agent..."
            className="min-h-24 resize-none border-0 bg-transparent px-4 py-4 text-[15px] text-foreground shadow-none outline-none placeholder:text-muted-foreground focus-visible:ring-0"
          />
          <div className="flex items-center gap-2 border-t border-border bg-muted/30 px-3 py-3">
            <Badge variant="outline" className="rounded-md">
              {draft.model}
            </Badge>
            <div className="ml-auto" />
            <Button
              type="button"
              size="icon-sm"
              onClick={onGenerate}
              disabled={!prompt.trim() || drafting}
              className="size-9 rounded-full"
              aria-label="Draft config"
            >
              {drafting ? <Loader2 className="size-4 animate-spin" /> : <ArrowUp className="size-4" />}
            </Button>
          </div>
        </div>
      </section>

      <section className="min-h-0">
        <TemplateBrowser
          selectedTemplateId={selectedTemplateId}
          onSelect={onTemplateSelect}
        />
      </section>
    </div>
  );
}

function ConfigStep({
  canCreate,
  configText,
  copied,
  draft,
  draftNotice,
  drafting,
  error,
  lastRequest,
  parsedError,
  prompt,
  saving,
  view,
  onConfigChange,
  onCopy,
  onCreate,
  onPromptChange,
  onRefine,
  onViewChange,
}: {
  canCreate: boolean;
  configText: string;
  copied: boolean;
  draft: AgentDraft;
  draftNotice: string | null;
  drafting: boolean;
  error: string | null;
  lastRequest: string;
  parsedError: string | null;
  prompt: string;
  saving: boolean;
  view: BuilderView;
  onConfigChange: (next: string) => void;
  onCopy: () => void;
  onCreate: () => void;
  onPromptChange: (next: string) => void;
  onRefine: () => void;
  onViewChange: (next: BuilderView) => void;
}) {
  return (
    <div className="grid min-h-[calc(100vh-6.5rem)] gap-6 px-4 py-6 lg:grid-cols-[minmax(360px,0.82fr)_minmax(560px,1.18fr)]">
      <section className="flex min-h-[560px] flex-col">
        <div className="flex flex-1 items-center justify-center">
          <div className="w-full max-w-2xl">
            <div className="ml-auto max-w-max rounded-lg bg-foreground px-4 py-3 text-sm text-background">
              {lastRequest || draft.name}
            </div>
            <div className="mt-8 flex flex-wrap gap-3">
              <Button type="button" onClick={onCreate} disabled={!canCreate || drafting}>
                {saving ? "Creating..." : "Create this agent"}
              </Button>
              <Button
                type="button"
                variant="secondary"
                onClick={() => document.getElementById("agent-config-refine")?.focus()}
              >
                Keep refining
              </Button>
            </div>
            {draftNotice && (
              <div className="mt-4 max-w-xl rounded-lg border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-sm text-amber-700 dark:text-amber-300">
                {draftNotice}
              </div>
            )}
            {(error || parsedError) && (
              <div className="mt-4 max-w-xl rounded-lg border border-destructive/20 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                {error ?? parsedError}
              </div>
            )}
          </div>
        </div>

        <div className="mx-auto w-full max-w-3xl overflow-hidden rounded-lg border border-border bg-card shadow-[0_18px_70px_rgba(15,23,42,0.10)]">
          <Textarea
            id="agent-config-refine"
            value={prompt}
            onChange={(event) => onPromptChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey && !drafting) {
                event.preventDefault();
                onRefine();
              }
            }}
            placeholder="Reply..."
            className="min-h-20 resize-none border-0 bg-transparent px-4 py-4 text-[15px] text-foreground shadow-none outline-none placeholder:text-muted-foreground focus-visible:ring-0"
          />
          <div className="flex items-center border-t border-border bg-muted/30 px-3 py-3">
            <div className="ml-auto" />
            <Button
              type="button"
              size="icon-sm"
              onClick={onRefine}
              disabled={!prompt.trim() || drafting}
              className="size-9 rounded-full"
              aria-label="Refine config"
            >
              {drafting ? <Loader2 className="size-4 animate-spin" /> : <ArrowUp className="size-4" />}
            </Button>
          </div>
        </div>
      </section>

      <section className="min-h-0">
        <div className="flex h-full min-h-[560px] flex-col overflow-hidden rounded-lg border border-[#343330] bg-[#2b2a28] text-[#f7f2e8] shadow-[0_18px_70px_rgba(15,23,42,0.16)]">
          <div className="flex shrink-0 items-center justify-between border-b border-white/10 px-4 py-3">
            <div className="flex items-center gap-1">
              <Button
                type="button"
                size="sm"
                variant={view === "config" ? "secondary" : "ghost"}
                onClick={() => onViewChange("config")}
                className={cn(
                  "h-8 text-[#c9c0b1] hover:bg-white/10 hover:text-white",
                  view === "config" && "bg-[#f4f1ea] text-[#1b1b1a] hover:bg-white",
                )}
              >
                <Code2 className="size-3.5" />
                Config
              </Button>
              <Button
                type="button"
                size="sm"
                variant={view === "preview" ? "secondary" : "ghost"}
                onClick={() => onViewChange("preview")}
                className={cn(
                  "h-8 text-[#c9c0b1] hover:bg-white/10 hover:text-white",
                  view === "preview" && "bg-[#f4f1ea] text-[#1b1b1a] hover:bg-white",
                )}
              >
                <FileSearch className="size-3.5" />
                Preview
              </Button>
            </div>
            <div className="flex items-center gap-2">
              {parsedError ? (
                <span className="flex items-center gap-1 text-xs text-red-300">
                  <XCircle className="size-3.5" />
                  Invalid
                </span>
              ) : (
                <span className="flex items-center gap-1 text-xs text-emerald-300">
                  <CheckCircle2 className="size-3.5" />
                  Ready
                </span>
              )}
              <Button
                type="button"
                size="icon-sm"
                variant="ghost"
                onClick={onCopy}
                className="text-[#c9c0b1] hover:bg-white/10 hover:text-white"
                aria-label="Copy config"
                title="Copy config"
              >
                <Clipboard className="size-4" />
              </Button>
            </div>
          </div>

          {view === "config" ? (
            <Textarea
              value={configText}
              onChange={(event) => onConfigChange(event.target.value)}
              spellCheck={false}
              className="min-h-0 flex-1 resize-none rounded-none border-0 bg-[#2b2a28] px-5 py-4 font-mono text-[13px] leading-6 text-[#e8b28c] shadow-none outline-none focus-visible:ring-0"
              aria-label="Agent YAML config"
            />
          ) : (
            <ConfigPreview draft={draft} />
          )}

          <div className="flex shrink-0 flex-wrap items-center gap-2 border-t border-white/10 px-4 py-3 text-xs text-[#c9c0b1]">
            <span className="font-mono">{scheduleLabel(draft.cron, draft.timezone)}</span>
            <span className="hidden text-white/20 sm:inline">/</span>
            <span className="font-mono">{draft.max_runtime_minutes} min max</span>
            {copied && <span className="ml-auto text-emerald-300">Copied</span>}
          </div>
        </div>
      </section>
    </div>
  );
}

function TemplateBrowser({
  selectedTemplateId,
  onSelect,
}: {
  selectedTemplateId: string;
  onSelect: (template: AgentTemplate) => void;
}) {
  const [query, setQuery] = useState("");
  const normalized = query.trim().toLowerCase();
  const templates = normalized
    ? AGENT_TEMPLATES.filter((template) =>
        [
          template.title,
          template.description,
          ...template.tags,
          template.draft.name,
        ]
          .join(" ")
          .toLowerCase()
          .includes(normalized),
      )
    : AGENT_TEMPLATES;

  return (
    <div className="flex h-full min-h-[560px] flex-col rounded-lg border border-border bg-card p-5 shadow-sm">
      <div className="mb-4">
        <h2 className="text-lg font-semibold text-foreground">Browse templates</h2>
        <div className="relative mt-4">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search templates"
            className="h-10 pl-9"
          />
        </div>
      </div>
      <div className="grid min-h-0 flex-1 gap-3 overflow-y-auto pr-1 sm:grid-cols-2">
        {templates.map((template) => {
          const Icon = TEMPLATE_ICONS[template.id] ?? Sparkles;
          const selected = template.id === selectedTemplateId;
          return (
            <button
              key={template.id}
              type="button"
              onClick={() => onSelect(template)}
              className={cn(
                "min-h-28 rounded-lg border border-border bg-background p-4 text-left transition hover:bg-muted/40",
                selected && "border-foreground ring-2 ring-foreground/10",
              )}
            >
              <div className="flex min-h-full flex-col">
                <div className="flex items-start gap-3">
                  <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-muted text-foreground">
                    <Icon className="size-4" />
                  </span>
                  <span className="min-w-0">
                    <span className="block text-sm font-semibold text-foreground">{template.title}</span>
                    <span className="mt-1 line-clamp-2 block text-xs leading-5 text-muted-foreground">
                      {template.description}
                    </span>
                  </span>
                </div>
                <div className="mt-auto flex flex-wrap gap-1.5 pt-4">
                  {template.tags.map((tag) => (
                    <span
                      key={tag}
                      className="rounded-md border border-border bg-muted/40 px-2 py-0.5 text-[11px] text-muted-foreground"
                    >
                      {tag}
                    </span>
                  ))}
                </div>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function ConfigPreview({ draft }: { draft: AgentDraft }) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
      <div className="grid gap-5">
        <div>
          <div className="text-xs uppercase text-[#9d9384]">Name</div>
          <div className="mt-1 text-xl font-semibold text-[#fffaf0]">{draft.name}</div>
          <p className="mt-2 max-w-2xl text-sm leading-6 text-[#c9c0b1]">{draft.description}</p>
        </div>

        <div className="grid gap-3 sm:grid-cols-2">
          <PreviewItem label="Model" value={draft.model} />
          <PreviewItem label="Runtime" value={draft.runtime} />
          <PreviewItem label="Schedule" value={scheduleLabel(draft.cron, draft.timezone)} />
          <PreviewItem label="Tools" value={draft.tools.map((tool) => tool.type).filter(Boolean).join(", ")} />
        </div>

        <div>
          <div className="text-xs uppercase text-[#9d9384]">System prompt</div>
          <pre className="mt-2 max-h-80 overflow-y-auto whitespace-pre-wrap rounded-lg border border-white/10 bg-black/15 p-3 font-mono text-[12px] leading-6 text-[#f0d3bd]">
            {draft.system || "No system prompt set."}
          </pre>
        </div>

        <div className="grid gap-3 sm:grid-cols-2">
          <TokenList label="Vault keys" values={draft.vault_keys} />
          <TokenList label="Skill IDs" values={draft.skill_ids} />
        </div>
      </div>
    </div>
  );
}

function PreviewItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-white/10 bg-black/10 p-3">
      <div className="text-xs uppercase text-[#9d9384]">{label}</div>
      <div className="mt-1 break-words font-mono text-xs text-[#f7f2e8]">{value || "Not set"}</div>
    </div>
  );
}

function TokenList({ label, values }: { label: string; values: string[] }) {
  return (
    <div className="rounded-lg border border-white/10 bg-black/10 p-3">
      <div className="text-xs uppercase text-[#9d9384]">{label}</div>
      {values.length === 0 ? (
        <div className="mt-2 text-xs text-[#c9c0b1]">None</div>
      ) : (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {values.map((value) => (
            <span
              key={value}
              className="rounded-md border border-white/10 bg-white/5 px-1.5 py-0.5 font-mono text-[11px] text-[#f7f2e8]"
            >
              {value}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
