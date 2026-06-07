"use client";

import { useState } from "react";
import { Eye, EyeOff, Info, Check, Loader2, Unplug } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { BrandIcon } from "@/components/brand-icons";
import { storeMcpUserCredential, deleteMcpUserCredential } from "@/lib/api";
import type { McpServer } from "@/lib/types";

export function IntegrationDialog({
  server,
  open,
  connected,
  onOpenChange,
  onChange,
}: {
  server: McpServer | null;
  open: boolean;
  connected: boolean;
  onOpenChange: (open: boolean) => void;
  onChange: () => void;
}) {
  const [apiKey, setApiKey] = useState("");
  const [reveal, setReveal] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!server) return null;

  const displayName = server.server_name ?? server.alias ?? server.server_id;
  const keyLabel = server.byok_description?.[0] ?? "API Key";
  const tools: string[] = server.allowed_tools ?? [];

  const reset = () => {
    setApiKey("");
    setReveal(false);
    setError(null);
  };

  const onSave = async () => {
    if (!apiKey.trim()) return;
    setSaving(true);
    setError(null);
    try {
      await storeMcpUserCredential(server.server_id, apiKey.trim());
      onChange();
      reset();
      onOpenChange(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const onDisconnect = async () => {
    setSaving(true);
    setError(null);
    try {
      await deleteMcpUserCredential(server.server_id);
      onChange();
      reset();
      onOpenChange(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) reset();
        onOpenChange(o);
      }}
    >
      <DialogContent className="sm:max-w-lg">
        <div className="flex items-start gap-3 pr-6">
          <div className="flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-border bg-muted/40">
            <BrandIcon id={server.server_id} className="size-6" />
          </div>
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <h2 className="text-base font-medium leading-none">{displayName}</h2>
              {connected && (
                <Badge variant="secondary" className="gap-1">
                  <Check className="size-3" />
                  Connected
                </Badge>
              )}
            </div>
            <p className="mt-1 text-sm text-muted-foreground">
              {server.description ?? ""}
            </p>
          </div>
        </div>

        {tools.length > 0 && (
          <div>
            <div className="mb-2 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
              Available tools ({tools.length})
            </div>
            <div className="flex flex-wrap gap-1.5 rounded-lg border border-border bg-muted/30 p-3">
              {tools.map((t) => (
                <Badge key={t} variant="outline" className="font-mono">
                  {t}
                </Badge>
              ))}
            </div>
          </div>
        )}

        <div className="flex items-start gap-2 rounded-lg border border-border bg-muted/30 p-3 text-sm text-muted-foreground">
          <Info className="mt-0.5 size-4 shrink-0" />
          <span>
            To use this service, please provide your {keyLabel} below.
            {server.byok_api_key_help_url && (
              <>
                {" "}
                <a
                  href={server.byok_api_key_help_url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="underline hover:text-foreground"
                >
                  Get your {keyLabel}
                </a>
              </>
            )}
          </span>
        </div>

        <div className="space-y-2">
          <label className="font-mono text-xs text-muted-foreground">
            {keyLabel}
          </label>
          <div className="relative">
            <Input
              type={reveal ? "text" : "password"}
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="Enter API key..."
              className="h-10 pr-9 font-mono"
              autoComplete="off"
              onKeyDown={(e) => {
                if (e.key === "Enter") void onSave();
              }}
            />
            <button
              type="button"
              onClick={() => setReveal((r) => !r)}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              aria-label={reveal ? "Hide API key" : "Show API key"}
            >
              {reveal ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
            </button>
          </div>

          {error && <div className="text-xs text-destructive">{error}</div>}

          <Button
            onClick={() => void onSave()}
            disabled={saving || !apiKey.trim()}
            className="w-full"
          >
            {saving ? (
              <>
                <Loader2 className="size-4 animate-spin" />
                Saving
              </>
            ) : connected ? (
              "Update API key"
            ) : (
              "Save"
            )}
          </Button>

          {connected && (
            <Button
              variant="ghost"
              onClick={() => void onDisconnect()}
              disabled={saving}
              className="w-full text-destructive hover:text-destructive"
            >
              <Unplug className="size-4" />
              Disconnect
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
