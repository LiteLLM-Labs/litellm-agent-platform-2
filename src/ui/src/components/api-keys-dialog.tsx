"use client";

import { useEffect, useState } from "react";
import { Check, Copy, Loader2, Plus, Trash2, X } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  createGatewayApiKey,
  deleteGatewayApiKey,
  listGatewayApiKeys,
  type CreatedGatewayApiKey,
  type GatewayApiKey,
} from "@/lib/api";

function formatTime(ts?: number | null): string {
  if (!ts) return "Never";
  return new Date(ts * 1000).toLocaleString();
}

export function ApiKeysPanel() {
  const [keys, setKeys] = useState<GatewayApiKey[] | null>(null);
  const [label, setLabel] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<CreatedGatewayApiKey | null>(null);

  const load = async () => {
    setKeys(await listGatewayApiKeys());
  };

  useEffect(() => {
    load().catch((error) => toast.error(error instanceof Error ? error.message : String(error)));
  }, []);

  const create = async () => {
    setCreating(true);
    setCreated(null);
    try {
      const key = await createGatewayApiKey(label.trim() || undefined);
      setCreated(key);
      setLabel("");
      setShowCreate(false);
      await load();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setCreating(false);
    }
  };

  const remove = async (id: string) => {
    setKeys((current) => current?.filter((key) => key.id !== id) ?? null);
    try {
      await deleteGatewayApiKey(id);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
      await load().catch(() => {});
    }
  };

  return (
    <section className="rounded-lg border border-border bg-card">
      <div className="flex flex-col gap-3 border-b border-border px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold">Keys</h3>
          <p className="mt-1 text-sm text-muted-foreground">
            View and manage gateway keys for CLI and agent access.
          </p>
        </div>
        <Button size="sm" onClick={() => setShowCreate(true)} disabled={showCreate}>
          <Plus className="size-4" />
          Make key
        </Button>
      </div>

      {showCreate && (
        <div className="border-b border-border bg-muted/20 px-4 py-3">
          <div className="flex flex-col gap-2 sm:flex-row">
            <Input
              value={label}
              onChange={(event) => setLabel(event.target.value)}
              placeholder="Label, optional"
              onKeyDown={(event) => {
                if (event.key === "Enter") create();
              }}
            />
            <div className="flex gap-2">
              <Button onClick={create} disabled={creating} className="shrink-0">
                {creating ? <Loader2 className="size-4 animate-spin" /> : <Plus className="size-4" />}
                Create
              </Button>
              <Button
                variant="ghost"
                size="icon"
                onClick={() => {
                  setShowCreate(false);
                  setLabel("");
                }}
                aria-label="Cancel key creation"
                title="Cancel"
              >
                <X className="size-4" />
              </Button>
            </div>
          </div>
        </div>
      )}

      {created && <CreatedKeyCard created={created} />}

      {keys === null ? (
        <div className="flex items-center gap-2 px-4 py-6 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" />
          Loading keys
        </div>
      ) : keys.length === 0 ? (
        <div className="px-4 py-8 text-sm text-muted-foreground">No keys yet.</div>
      ) : (
        <div className="divide-y divide-border">
          {keys.map((key) => (
            <div key={key.id} className="grid gap-3 px-4 py-3 sm:grid-cols-[minmax(0,1fr)_180px_180px_auto] sm:items-center">
              <div className="min-w-0">
                <div className="truncate text-sm font-medium">{key.label || "Untitled key"}</div>
                <div className="mt-1 truncate font-mono text-xs text-muted-foreground">{key.id}</div>
              </div>
              <div className="text-xs text-muted-foreground">
                <span className="sm:hidden">Created </span>
                {formatTime(key.created_at)}
              </div>
              <div className="text-xs text-muted-foreground">
                <span className="sm:hidden">Last used </span>
                {formatTime(key.last_used_at)}
              </div>
              <Button
                variant="ghost"
                size="icon-sm"
                className="justify-self-start text-destructive hover:text-destructive sm:justify-self-end"
                onClick={() => remove(key.id)}
                aria-label="Delete API key"
                title="Delete key"
              >
                <Trash2 className="size-4" />
              </Button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function CreatedKeyCard({ created }: { created: CreatedGatewayApiKey }) {
  return (
    <div className="border-b border-border bg-emerald-500/10 px-4 py-3">
      <div className="mb-2 text-sm font-medium">New key created</div>
      <div className="flex items-center gap-2 rounded-lg border border-border bg-background px-3 py-2">
        <code className="min-w-0 flex-1 overflow-x-auto font-mono text-sm">{created.key}</code>
        <CopyButton value={created.key} label="Copy API key" />
      </div>
      <p className="mt-2 text-xs text-muted-foreground">
        Copy it now. It will not be shown again.
      </p>
    </div>
  );
}

function CopyButton({ value, label }: { value: string; label: string }) {
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };

  return (
    <Button variant="ghost" size="icon-sm" onClick={copy} aria-label={label} title={label}>
      {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
    </Button>
  );
}
