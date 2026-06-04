"use client";

import { useEffect, useMemo, useState } from "react";
import { ChartNoAxesCombined } from "lucide-react";

import { BrandIcon } from "@/components/brand-icons";
import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { listAgentUsage } from "@/lib/api";
import type { AgentUsageSummary } from "@/lib/types";

const AGENTS = [
  { key: "claude-code", name: "Claude Code", icon: "claude" },
  { key: "codex", name: "Codex", icon: "codex" },
];

function money(value: number): string {
  if (value <= 0) return "$0.00";
  if (value < 0.01) return `$${value.toFixed(4)}`;
  return `$${value.toFixed(2)}`;
}

function compact(value: number): string {
  return new Intl.NumberFormat(undefined, { notation: "compact" }).format(value);
}

function timeAgo(ts?: number | null): string {
  if (!ts) return "Never";
  const secs = Math.max(0, Math.floor((Date.now() - ts) / 1000));
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

export default function UsagePage() {
  const [usage, setUsage] = useState<AgentUsageSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const rows = await listAgentUsage();
        if (!cancelled) {
          setUsage(rows);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    };
    load();
    const t = setInterval(load, 5000);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, []);

  const rows = useMemo(() => {
    const byKey = new Map((usage ?? []).map((row) => [row.agent_key, row]));
    return AGENTS.map((agent) => ({
      ...agent,
      usage: byKey.get(agent.key) ?? {
        agent_key: agent.key,
        request_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        cost_usd: 0,
        last_used_at: null,
      },
    }));
  }, [usage]);

  const totalCost = rows.reduce((sum, row) => sum + row.usage.cost_usd, 0);
  const totalRequests = rows.reduce((sum, row) => sum + row.usage.request_count, 0);
  const totalTokens = rows.reduce(
    (sum, row) => sum + row.usage.input_tokens + row.usage.output_tokens,
    0,
  );

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
          <div className="flex items-center gap-2">
            <ChartNoAxesCombined className="size-4 text-muted-foreground" />
            <h1 className="text-sm font-semibold">Usage</h1>
          </div>
          <ThemeToggle />
        </header>

        <main className="flex-1 overflow-y-auto px-4 py-6">
          <div className="mx-auto flex max-w-6xl flex-col gap-4">
            <div className="grid gap-3 md:grid-cols-3">
              <Card size="sm" className="rounded-lg">
                <CardHeader>
                  <CardTitle>Cost</CardTitle>
                </CardHeader>
                <CardContent className="text-2xl font-semibold">{money(totalCost)}</CardContent>
              </Card>
              <Card size="sm" className="rounded-lg">
                <CardHeader>
                  <CardTitle>Requests</CardTitle>
                </CardHeader>
                <CardContent className="text-2xl font-semibold">
                  {totalRequests.toLocaleString()}
                </CardContent>
              </Card>
              <Card size="sm" className="rounded-lg">
                <CardHeader>
                  <CardTitle>Tokens</CardTitle>
                </CardHeader>
                <CardContent className="text-2xl font-semibold">{compact(totalTokens)}</CardContent>
              </Card>
            </div>

            <Card className="rounded-lg">
              <CardHeader>
                <CardTitle>Usage by Agents</CardTitle>
              </CardHeader>
              <CardContent>
                {error && <div className="pb-3 text-sm text-destructive">{error}</div>}
                {!usage && !error && (
                  <div className="pb-3 text-sm text-muted-foreground">Loading...</div>
                )}
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Agent</TableHead>
                      <TableHead className="text-right">Cost</TableHead>
                      <TableHead className="text-right">Requests</TableHead>
                      <TableHead className="text-right">Input</TableHead>
                      <TableHead className="text-right">Output</TableHead>
                      <TableHead className="text-right">Last Used</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {rows.map((row) => (
                      <TableRow key={row.key}>
                        <TableCell>
                          <div className="flex items-center gap-3">
                            <div className="flex size-9 items-center justify-center rounded-md border border-border bg-background">
                              <BrandIcon id={row.icon} className="size-5" />
                            </div>
                            <div>
                              <div className="font-medium">{row.name}</div>
                              <div className="text-xs text-muted-foreground">{row.key}</div>
                            </div>
                          </div>
                        </TableCell>
                        <TableCell className="text-right font-medium">
                          {money(row.usage.cost_usd)}
                        </TableCell>
                        <TableCell className="text-right">
                          {row.usage.request_count.toLocaleString()}
                        </TableCell>
                        <TableCell className="text-right">
                          {row.usage.input_tokens.toLocaleString()}
                        </TableCell>
                        <TableCell className="text-right">
                          {row.usage.output_tokens.toLocaleString()}
                        </TableCell>
                        <TableCell className="text-right text-muted-foreground">
                          {timeAgo(row.usage.last_used_at)}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          </div>
        </main>
      </div>
    </div>
  );
}
