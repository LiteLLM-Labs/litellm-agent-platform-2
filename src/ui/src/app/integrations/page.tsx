"use client";

import { useEffect, useMemo, useState } from "react";
import { Search, Check, Puzzle } from "lucide-react";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { IntegrationDialog } from "@/components/integration-dialog";
import { BrandIcon } from "@/components/brand-icons";
import { listIntegrationKeys } from "@/lib/api";
import {
  integrationsByCategory,
  type Integration,
} from "@/lib/integrations";

export default function IntegrationsPage() {
  const [connected, setConnected] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [active, setActive] = useState<Integration | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);

  const refresh = async () => {
    const keys = await listIntegrationKeys();
    setConnected(new Set(keys));
  };

  useEffect(() => {
    refresh();
  }, []);

  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    return integrationsByCategory()
      .map(([cat, items]) => {
        const filtered = q
          ? items.filter(
              (it) =>
                it.name.toLowerCase().includes(q) ||
                it.description.toLowerCase().includes(q),
            )
          : items;
        return [cat, filtered] as [string, Integration[]];
      })
      .filter(([, items]) => items.length > 0);
  }, [query]);

  const openDialog = (it: Integration) => {
    setActive(it);
    setDialogOpen(true);
  };

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar />
      <div className="flex flex-1 flex-col min-w-0">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
          <div className="flex items-center gap-2">
            <Puzzle className="size-4" />
            <span className="text-sm font-semibold">Integrations</span>
          </div>
          <ThemeToggle />
        </header>

        <main id="main-content" className="flex-1 overflow-y-auto">
          <div className="mx-auto w-full max-w-4xl px-6 py-6">
            <div className="mb-6">
              <h1 className="text-xl font-semibold tracking-tight">Connect your tools</h1>
              <p className="text-sm text-muted-foreground">
                Each integration is a managed MCP server. Add your API key to make
                its tools available to your agents.
              </p>
            </div>

            <div className="relative mb-6 max-w-sm">
              <Search className="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search…"
                className="h-9 pl-8"
              />
            </div>

            {groups.length === 0 && (
              <div className="rounded-xl border border-dashed border-border py-12 text-center">
                <Puzzle className="mx-auto mb-2 size-6 text-muted-foreground" />
                <p className="text-sm text-muted-foreground">
                  No integrations match &ldquo;{query}&rdquo;.
                </p>
                <button
                  onClick={() => setQuery("")}
                  className="mt-3 text-xs text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                >
                  Clear search
                </button>
              </div>
            )}

            <div className="space-y-8">
              {groups.map(([cat, items]) => (
                <section key={cat}>
                  <div className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
                    {cat}
                  </div>
                  <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                    {items.map((it) => {
                      const isConnected = connected.has(it.envKey);
                      return (
                        <div
                          key={it.id}
                          className="flex items-start gap-3 rounded-xl border border-border bg-card p-4 transition-colors hover:border-primary/30"
                        >
                          <div className="flex size-9 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-border bg-muted/40">
                            <BrandIcon id={it.id} className="size-5" />
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="font-medium leading-none">{it.name}</div>
                            <p className="mt-1.5 line-clamp-2 text-xs text-muted-foreground">
                              {it.description}
                            </p>
                          </div>
                          <Button
                            size="sm"
                            variant={isConnected ? "secondary" : "outline"}
                            onClick={() => openDialog(it)}
                          >
                            {isConnected ? (
                              <>
                                <Check className="size-3.5" />
                                Connected
                              </>
                            ) : (
                              "Connect"
                            )}
                          </Button>
                        </div>
                      );
                    })}
                  </div>
                </section>
              ))}
            </div>
          </div>
        </main>
      </div>

      <IntegrationDialog
        integration={active}
        open={dialogOpen}
        connected={active ? connected.has(active.envKey) : false}
        onOpenChange={setDialogOpen}
        onChange={refresh}
      />
    </div>
  );
}
