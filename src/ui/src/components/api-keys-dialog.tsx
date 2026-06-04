"use client";

import { useEffect, useState } from "react";
import { Check, Copy, KeyRound, Loader2, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { BrandIcon } from "@/components/brand-icons";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  createGatewayApiKey,
  deleteGatewayApiKey,
  listGatewayApiKeys,
} from "@/lib/api";
import type { CreatedGatewayApiKey, GatewayApiKey } from "@/lib/api";

function formatDate(seconds?: number | null): string {
  if (!seconds) return "never used";
  return new Date(seconds * 1000).toLocaleString();
}

function gatewayOrigin(): string {
  if (typeof window === "undefined") return "http://localhost:4000";
  return window.location.origin;
}

function copyText(text: string): void {
  navigator.clipboard?.writeText(text).then(
    () => toast.success("Copied"),
    () => toast.error("Copy failed"),
  );
}

export function ApiKeysButton() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button
        variant="outline"
        size="sm"
        className="h-8 gap-1.5"
        onClick={() => setOpen(true)}
      >
        <KeyRound className="size-4" />
        <span className="hidden sm:inline">API Keys</span>
      </Button>
      <ApiKeysDialog open={open} onOpenChange={setOpen} />
    </>
  );
}

function ApiKeysDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [keys, setKeys] = useState<GatewayApiKey[] | null>(null);
  const [label, setLabel] = useState("");
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState<CreatedGatewayApiKey | null>(null);
  const origin = gatewayOrigin();

  const refresh = async () => {
    setKeys(await listGatewayApiKeys());
  };

  useEffect(() => {
    if (!open) return;
    refresh().catch((error) => toast.error(error instanceof Error ? error.message : String(error)));
  }, [open]);

  const onCreate = async () => {
    setBusy(true);
    try {
      const key = await createGatewayApiKey(label);
      setCreated(key);
      setLabel("");
      await refresh();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const onDelete = async (id: string) => {
    setKeys((prev) => prev?.filter((key) => key.id !== id) ?? null);
    try {
      await deleteGatewayApiKey(id);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
      await refresh().catch(() => {});
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[min(780px,calc(100vh-2rem))] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>API Keys</DialogTitle>
          <DialogDescription>
            Create gateway keys for local CLIs and coding agents.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4">
          <div className="rounded-lg border border-border bg-card p-3">
            <div className="flex flex-col gap-2 sm:flex-row">
              <Input
                value={label}
                onChange={(event) => setLabel(event.target.value)}
                placeholder="Label, optional"
                onKeyDown={(event) => {
                  if (event.key === "Enter") onCreate();
                }}
              />
              <Button onClick={onCreate} disabled={busy} className="shrink-0">
                {busy ? <Loader2 className="size-4 animate-spin" /> : <Plus className="size-4" />}
                Create API Key
              </Button>
            </div>
          </div>

          {created && <CreatedKeyPanel created={created} origin={origin} />}

          <div className="rounded-lg border border-border">
            <div className="border-b border-border px-4 py-3 text-sm font-medium">
              Existing keys
            </div>
            {keys === null ? (
              <div className="flex items-center gap-2 px-4 py-6 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" />
                Loading
              </div>
            ) : keys.length === 0 ? (
              <div className="px-4 py-6 text-sm text-muted-foreground">
                No API keys yet.
              </div>
            ) : (
              <div className="divide-y divide-border">
                {keys.map((key) => (
                  <div key={key.id} className="flex items-center justify-between gap-3 px-4 py-3">
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium">
                        {key.label || "Untitled key"}
                      </div>
                      <div className="mt-1 truncate font-mono text-xs text-muted-foreground">
                        {key.id} · {formatDate(key.last_used_at)}
                      </div>
                    </div>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="shrink-0 text-destructive hover:text-destructive"
                      onClick={() => onDelete(key.id)}
                      aria-label="Delete API key"
                    >
                      <Trash2 className="size-4" />
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function CreatedKeyPanel({
  created,
  origin,
}: {
  created: CreatedGatewayApiKey;
  origin: string;
}) {
  const claudeCommand = `ANTHROPIC_BASE_URL="${origin}/v1" ANTHROPIC_AUTH_TOKEN="${created.key}" claude`;
  const codexCommand = `OPENAI_BASE_URL="${origin}/v1" OPENAI_API_KEY="${created.key}" codex`;
  const agentPrompt = `You have access to LiteLLM's Rust AI gateway at ${origin}. Use this API key as a bearer token: ${created.key}

Start by checking what you can access:
- Providers and model IDs: GET ${origin}/v1/models
- Full gateway capabilities: GET ${origin}/api/capabilities
- OpenAPI schema and endpoints: GET ${origin}/openapi.json
- MCP servers: inspect "mcp_servers" from /api/capabilities, then call ${origin}/mcp or ${origin}/mcp/{server_id}
- Managed agents: inspect "agents" from /api/capabilities, then call POST ${origin}/api/agents/{agent_id}/run when available`;

  return (
    <div className="grid gap-3 rounded-lg border border-amber-500/40 bg-amber-500/5 p-3">
      <div className="flex items-start gap-2 text-sm text-amber-700 dark:text-amber-300">
        <Check className="mt-0.5 size-4 shrink-0" />
        <span>Copy this key now. It will not be displayed again after closing.</span>
      </div>

      <CopyBlock label="Your API key" value={created.key} />

      <div className="grid gap-3 md:grid-cols-2">
        <CliCard
          title="Start Claude"
          icon="claude"
          command={claudeCommand}
        />
        <CliCard
          title="Start Codex"
          icon="codex"
          command={codexCommand}
        />
      </div>

      <CopyBlock label="Prompt for AI agents" value={agentPrompt} tall />
    </div>
  );
}

function CliCard({
  title,
  icon,
  command,
}: {
  title: string;
  icon: string;
  command: string;
}) {
  return (
    <div className="rounded-lg border border-border bg-background p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-sm font-medium">
          <BrandIcon id={icon} className="size-5" />
          {title}
        </div>
        <CopyButton value={command} label={`Copy ${title} command`} />
      </div>
      <pre className="overflow-x-auto rounded-md bg-muted p-2 font-mono text-xs text-muted-foreground">
        {command}
      </pre>
    </div>
  );
}

function CopyBlock({
  label,
  value,
  tall,
}: {
  label: string;
  value: string;
  tall?: boolean;
}) {
  return (
    <div className="rounded-lg border border-border bg-background p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="text-sm font-medium">{label}</div>
        <CopyButton value={value} label={`Copy ${label}`} />
      </div>
      <pre
        className={`overflow-auto rounded-md bg-muted p-3 font-mono text-xs text-muted-foreground ${tall ? "max-h-48 whitespace-pre-wrap" : "whitespace-pre"}`}
      >
        {value}
      </pre>
    </div>
  );
}

function CopyButton({ value, label }: { value: string; label: string }) {
  return (
    <Button
      variant="ghost"
      size="icon-sm"
      onClick={() => copyText(value)}
      aria-label={label}
      title={label}
    >
      <Copy className="size-4" />
    </Button>
  );
}
