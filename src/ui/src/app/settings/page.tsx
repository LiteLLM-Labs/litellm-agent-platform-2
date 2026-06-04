"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { Bot, Check, KeyRound, Plus, ServerCog, X } from "lucide-react";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { BrandIcon } from "@/components/brand-icons";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  clearHarnessServerKey,
  clearHarnessServerUrl,
  deleteProvider,
  getHarnessServerKey,
  getHarnessServerUrl,
  listProviders,
  normalizeHarnessServerUrl,
  saveProvider,
  setHarnessServerKey,
  setHarnessServerUrl,
  testHarnessServer,
  type AvailableProvider,
  type ConnectedProvider,
} from "@/lib/api";

type Step = "catalog" | "configure" | "connected";

export default function SettingsPage() {
  const [step, setStep] = useState<Step>("catalog");
  const [availableProviders, setAvailableProviders] = useState<AvailableProvider[]>([]);
  const [selectedProviderId, setSelectedProviderId] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [connectedProviders, setConnectedProviders] = useState<ConnectedProvider[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [harnessUrl, setHarnessUrl] = useState("");
  const [harnessKey, setHarnessKey] = useState("");
  const [savedHarnessUrl, setSavedHarnessUrl] = useState("");
  const [harnessTesting, setHarnessTesting] = useState(false);
  const [harnessStatus, setHarnessStatus] = useState<{
    tone: "success" | "error" | "muted";
    text: string;
  } | null>(null);

  const selectedProvider = useMemo(
    () => availableProviders.find((provider) => provider.id === selectedProviderId),
    [availableProviders, selectedProviderId],
  );
  const selectedConnectedProvider = useMemo(
    () => connectedProviders.find((provider) => provider.id === selectedProviderId) ?? null,
    [connectedProviders, selectedProviderId],
  );
  const connected = Boolean(selectedConnectedProvider);

  const refreshProviders = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await listProviders();
      setAvailableProviders(data.available_providers);
      const provider = data.available_providers[0];
      if (provider) {
        setSelectedProviderId(provider.id);
      }
      setConnectedProviders(data.connected_providers);
      const connected = data.connected_providers[0] ?? null;
      if (connected) {
        setBaseUrl(connected.api_base);
        setStep("connected");
      } else if (provider) {
        setBaseUrl(provider.default_base_url);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load providers");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshProviders();
  }, [refreshProviders]);

  useEffect(() => {
    const url = getHarnessServerUrl();
    setHarnessUrl(url);
    setSavedHarnessUrl(url);
    setHarnessKey(getHarnessServerKey());
  }, []);

  const maskedKey = useMemo(() => {
    if (selectedConnectedProvider) return selectedConnectedProvider.masked_api_key;
    const trimmed = apiKey.trim();
    if (!trimmed) return "No API key";
    if (trimmed.length <= 10) return "Configured";
    return `${trimmed.slice(0, 7)}...${trimmed.slice(-4)}`;
  }, [apiKey, selectedConnectedProvider]);

  const connect = async () => {
    if (!selectedProvider || !apiKey.trim() || !baseUrl.trim()) return;
    setSaving(true);
    setError(null);
    try {
      const data = await saveProvider({
        providerId: selectedProvider.id,
        apiKey,
        apiBase: baseUrl,
      });
      const connected = data.connected_providers.find(
        (provider) => provider.id === selectedProvider.id,
      );
      setConnectedProviders(data.connected_providers);
      setApiKey("");
      setStep(connected ? "connected" : "catalog");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save provider");
    } finally {
      setSaving(false);
    }
  };

  const disconnect = async (providerId: string) => {
    const provider = availableProviders.find((entry) => entry.id === providerId);
    setSaving(true);
    setError(null);
    try {
      await deleteProvider(providerId);
      setConnectedProviders((providers) =>
        providers.filter((connectedProvider) => connectedProvider.id !== providerId),
      );
      if (selectedProviderId === providerId) {
        setApiKey("");
        setBaseUrl(provider?.default_base_url ?? "");
        setStep("catalog");
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to disconnect provider");
    } finally {
      setSaving(false);
    }
  };

  const testHarness = async () => {
    const normalized = normalizeHarnessServerUrl(harnessUrl);
    if (harnessUrl.trim() && !normalized) {
      setHarnessStatus({ tone: "error", text: "Enter a valid http:// or https:// URL." });
      return;
    }
    setHarnessTesting(true);
    setHarnessStatus(null);
    try {
      const result = await testHarnessServer(normalized, harnessKey);
      if (result.ok) {
        setHarnessStatus({
          tone: "success",
          text:
            result.mode === "remote"
              ? `Connected to ${result.base}.`
              : "Using LAP local harness routing.",
        });
      } else {
        setHarnessStatus({
          tone: "error",
          text: result.error ?? `Harness server returned HTTP ${result.status ?? "error"}.`,
        });
      }
    } finally {
      setHarnessTesting(false);
    }
  };

  const saveHarness = () => {
    const normalized = normalizeHarnessServerUrl(harnessUrl);
    if (harnessUrl.trim() && !normalized) {
      setHarnessStatus({ tone: "error", text: "Enter a valid http:// or https:// URL." });
      return;
    }
    const saved = setHarnessServerUrl(normalized);
    setHarnessServerKey(harnessKey);
    setHarnessUrl(saved);
    setSavedHarnessUrl(saved);
    setHarnessStatus({
      tone: "success",
      text: saved ? `Session calls now route through ${saved}.` : "Session calls now use LAP local routing.",
    });
  };

  const useLocalHarness = () => {
    clearHarnessServerUrl();
    clearHarnessServerKey();
    setHarnessUrl("");
    setHarnessKey("");
    setSavedHarnessUrl("");
    setHarnessStatus({ tone: "muted", text: "Session calls now use LAP local routing." });
  };

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
          <div className="flex items-center gap-2">
            <ServerCog className="size-4 text-muted-foreground" />
            <h1 className="text-sm font-semibold">Settings</h1>
          </div>
          <ThemeToggle />
        </header>

        <main className="flex-1 overflow-y-auto">
          <div className="mx-auto flex max-w-5xl flex-col gap-5 px-4 py-6">
            <section className="grid gap-2">
              <div className="flex items-center justify-between gap-3">
                <h2 className="text-lg font-semibold">Harness Server</h2>
                <Badge variant={savedHarnessUrl ? "secondary" : "outline"} className="text-[10px]">
                  {savedHarnessUrl ? "Lite-Harness remote" : "LAP local"}
                </Badge>
              </div>
              <Card className="p-4">
                <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_260px]">
                  <div className="grid gap-3">
                    <p className="text-sm text-muted-foreground">
                      Route chat sessions through a running Lite-Harness server.
                    </p>
                    <div className="grid gap-1.5">
                      <Label htmlFor="harness-server-url">Server URL</Label>
                      <Input
                        id="harness-server-url"
                        value={harnessUrl}
                        onChange={(event) => setHarnessUrl(event.target.value)}
                        placeholder="http://127.0.0.1:4096"
                        className="font-mono text-xs"
                      />
                    </div>
                    <div className="grid gap-1.5">
                      <Label htmlFor="harness-server-key">Master key</Label>
                      <div className="relative">
                        <KeyRound className="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                        <Input
                          id="harness-server-key"
                          type="password"
                          value={harnessKey}
                          onChange={(event) => setHarnessKey(event.target.value)}
                          placeholder="Optional"
                          className="pl-8 font-mono text-xs"
                        />
                      </div>
                    </div>
                  </div>

                  <div className="grid content-start gap-3 border-t border-border pt-4 lg:border-l lg:border-t-0 lg:pl-4 lg:pt-0">
                    <div className="grid gap-2 text-xs">
                      <div className="flex items-center justify-between gap-3 border-b border-border pb-2">
                        <span className="text-muted-foreground">Mode</span>
                        <span className="font-mono text-foreground">
                          {savedHarnessUrl ? "remote" : "local"}
                        </span>
                      </div>
                      <div className="flex items-center justify-between gap-3 border-b border-border pb-2">
                        <span className="text-muted-foreground">Sessions</span>
                        <span className="font-mono text-foreground">
                          {savedHarnessUrl ? "proxy" : "LAP"}
                        </span>
                      </div>
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-muted-foreground">Events</span>
                        <span className="font-mono text-foreground">
                          {savedHarnessUrl ? "proxy SSE" : "LAP SSE"}
                        </span>
                      </div>
                    </div>
                    {savedHarnessUrl && (
                      <p className="break-all font-mono text-[11px] text-muted-foreground">
                        {savedHarnessUrl}
                      </p>
                    )}
                  </div>
                </div>

                {harnessStatus && (
                  <p
                    className={`mt-4 text-xs ${
                      harnessStatus.tone === "error"
                        ? "text-destructive"
                        : harnessStatus.tone === "success"
                          ? "text-emerald-600"
                          : "text-muted-foreground"
                    }`}
                  >
                    {harnessStatus.text}
                  </p>
                )}

                <div className="mt-4 flex flex-wrap justify-end gap-2">
                  {savedHarnessUrl && (
                    <Button variant="outline" size="sm" onClick={useLocalHarness}>
                      <X className="size-3.5" />
                      Use local LAP
                    </Button>
                  )}
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={testHarness}
                    disabled={harnessTesting}
                  >
                    <ServerCog className="size-3.5" />
                    {harnessTesting ? "Testing..." : "Test"}
                  </Button>
                  <Button size="sm" onClick={saveHarness}>
                    <Check className="size-3.5" />
                    Save
                  </Button>
                </div>
              </Card>
            </section>

            <div className="flex flex-col gap-1">
              <h2 className="text-lg font-semibold">AI Providers</h2>
              <p className="text-sm text-muted-foreground">
                Connect provider credentials before assigning models to agents.
              </p>
              {loading && <p className="text-xs text-muted-foreground">Loading providers...</p>}
              {error && <p className="text-xs text-destructive">{error}</p>}
            </div>

            {connectedProviders.length > 0 && (
              <section className="grid gap-2">
                <h3>Connected providers</h3>
                <Card className="grid gap-3 p-4">
                  {connectedProviders.map((provider) => (
                    <div
                      key={provider.id}
                      className="flex items-center justify-between gap-4"
                    >
                      <div className="flex min-w-0 items-center gap-3">
                        <ProviderLogo providerId={provider.id} />
                        <div className="min-w-0">
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="font-medium">{provider.name}</span>
                            <Badge variant="secondary" className="text-[10px]">
                              API key
                            </Badge>
                            <Badge variant="outline" className="text-[10px]">
                              {provider.api_base}
                            </Badge>
                          </div>
                          <p className="mt-1 font-mono text-xs text-muted-foreground">
                            {provider.masked_api_key}
                          </p>
                        </div>
                      </div>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => disconnect(provider.id)}
                        disabled={saving}
                      >
                        <X className="size-3.5" />
                        Disconnect
                      </Button>
                    </div>
                  ))}
                </Card>
              </section>
            )}

            <section className="grid gap-2">
              <div className="flex items-center justify-between gap-3">
                <h3>Available providers</h3>
                <Badge variant="outline" className="text-[10px]">
                  Rust proxy catalog
                </Badge>
              </div>
              <Card className="overflow-hidden p-0">
                {availableProviders.map((provider) => (
                  <button
                    key={provider.id}
                    type="button"
                    className="flex w-full items-center justify-between gap-4 px-4 py-4 text-left transition-colors hover:bg-muted/50"
                    onClick={() => {
                      setSelectedProviderId(provider.id);
                      const connectedProvider = connectedProviders.find(
                        (connected) => connected.id === provider.id,
                      );
                      setBaseUrl(connectedProvider?.api_base ?? provider.default_base_url);
                      setStep(connectedProvider ? "connected" : "configure");
                    }}
                  >
                    <div className="flex min-w-0 items-center gap-3">
                      <ProviderLogo providerId={provider.id} />
                      <div className="min-w-0">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-medium">{provider.name}</span>
                          <Badge variant="secondary" className="text-[10px]">
                            Available
                          </Badge>
                        </div>
                        <p className="mt-1 text-sm text-muted-foreground">
                          {provider.description}
                        </p>
                      </div>
                    </div>
                    <span className="inline-flex h-7 shrink-0 items-center justify-center gap-1 rounded-lg border border-border bg-background px-2.5 text-[0.8rem] font-medium shadow-sm">
                      <Plus className="size-3.5" />
                      Connect
                    </span>
                  </button>
                ))}
              </Card>
            </section>

            {step !== "catalog" && selectedProvider && (
              <section className="grid gap-2">
                <h3>
                  {selectedConnectedProvider
                    ? "Provider details"
                    : `Connect ${selectedProvider.name}`}
                </h3>
                <Card className="p-4">
                  <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_280px]">
                    <div className="grid gap-4">
                      <div className="flex items-center gap-3">
                        <ProviderLogo providerId={selectedProvider.id} large />
                        <div>
                          <div className="font-medium">{selectedProvider.name}</div>
                          <p className="text-sm text-muted-foreground">
                            Add your provider API key and base URL.
                          </p>
                        </div>
                      </div>

                      <div className="grid gap-1.5">
                        <Label htmlFor="provider-key">{selectedProvider.name} API key</Label>
                        <div className="relative">
                          <KeyRound className="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                          <Input
                            id="provider-key"
                            type="password"
                            value={apiKey}
                            onChange={(event) => setApiKey(event.target.value)}
                            placeholder="Provider API key"
                            className="pl-8 font-mono text-xs"
                          />
                        </div>
                      </div>

                      <div className="grid gap-1.5">
                        <Label htmlFor="provider-base-url">{selectedProvider.name} base URL</Label>
                        <Input
                          id="provider-base-url"
                          value={baseUrl}
                          onChange={(event) => setBaseUrl(event.target.value)}
                          placeholder={selectedProvider.default_base_url}
                          className="font-mono text-xs"
                        />
                      </div>
                    </div>

                    <div className="rounded-lg border border-border bg-muted/30 p-3">
                      <div className="flex items-center gap-2 text-sm font-medium">
                        <Bot className="size-4" />
                        Agent routing
                      </div>
                      <div className="mt-3 space-y-3 text-xs text-muted-foreground">
                        <div className="flex items-center justify-between gap-3 border-b border-border pb-2">
                          <span>Provider</span>
                          <span className="text-foreground">{selectedProvider.name}</span>
                        </div>
                        <div className="flex items-center justify-between gap-3 border-b border-border pb-2">
                          <span>Models</span>
                          <span className="font-mono text-foreground">
                            {`${selectedProvider.id}/*`}
                          </span>
                        </div>
                        <div className="flex items-center justify-between gap-3">
                          <span>Status</span>
                          <span className="inline-flex items-center gap-1 text-foreground">
                            {connected && <Check className="size-3" />}
                            {connected ? "Connected" : "Ready"}
                          </span>
                        </div>
                      </div>
                    </div>
                  </div>

                  <div className="mt-4 flex justify-end gap-2">
                    <Button variant="outline" size="sm" onClick={() => setStep("catalog")}>
                      Cancel
                    </Button>
                    <Button
                      size="sm"
                      onClick={connect}
                      disabled={saving || !selectedProvider || !apiKey.trim() || !baseUrl.trim()}
                    >
                      <Check className="size-3.5" />
                      {saving ? "Saving..." : "Save provider"}
                    </Button>
                  </div>
                </Card>
              </section>
            )}
          </div>
        </main>
      </div>
    </div>
  );
}

function ProviderLogo({
  providerId,
  large = false,
}: {
  providerId: string;
  large?: boolean;
}) {
  return (
    <span
      className={`flex shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground shadow-sm ${
        large ? "size-11" : "size-9"
      }`}
    >
      <BrandIcon id={providerId} className={large ? "size-7" : "size-5"} />
    </span>
  );
}
