"use client";

import { useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import {
  AlertTriangle,
  BarChart3,
  Check,
  Clock3,
  Copy,
  DollarSign,
  RefreshCw,
  Search,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { getSpendLog, listSpendLogs } from "@/lib/api";
import type { SpendLog } from "@/lib/types";

const STATUS_OPTIONS = ["all", "success", "error"];

function formatCost(value: number | null | undefined): string {
  return `$${(value ?? 0).toFixed(8)}`;
}

function formatDate(value: string | null | undefined): string {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit",
  }).format(date);
}

function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return "-";
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(3)} s`;
}

function prettyJson(value: unknown): string {
  if (value == null) return "{}";
  return typeof value === "string" ? value : JSON.stringify(value, null, 2);
}

function errorInfo(log: SpendLog | null): Record<string, unknown> | null {
  const info = log?.metadata?.error_information;
  return info && typeof info === "object" && !Array.isArray(info)
    ? (info as Record<string, unknown>)
    : null;
}

export default function ObservabilityLogsPage() {
  const [logs, setLogs] = useState<SpendLog[]>([]);
  const [selected, setSelected] = useState<SpendLog | null>(null);
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState("all");
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = async (silent = false) => {
    if (silent) setRefreshing(true);
    else setLoading(true);
    try {
      const next = await listSpendLogs({ q: query, status, limit: 100 });
      setLogs(next);
      setError(null);
      if (next.length === 0) {
        setSelected(null);
        return;
      }
      const current = selected?.request_id;
      const pick = next.find((item) => item.request_id === current) ?? next[0];
      setSelected(await getSpendLog(pick.request_id));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  };

  useEffect(() => {
    load();
    const timer = setInterval(() => load(true), 10_000);
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, status]);

  const totals = useMemo(
    () =>
      logs.reduce(
        (acc, log) => ({
          cost: acc.cost + (log.spend ?? 0),
          tokens: acc.tokens + (log.total_tokens ?? 0),
          errors: acc.errors + (log.status === "error" ? 1 : 0),
        }),
        { cost: 0, tokens: 0, errors: 0 },
      ),
    [logs],
  );
  const selectedError = errorInfo(selected);

  return (
    <div className="flex h-screen bg-[#f5f5f7] text-[#1d1d1f]">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-[#d7d7dc] bg-white px-5">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <BarChart3 className="size-4 text-[#53657d]" />
              <h1 className="truncate text-[15px] font-semibold">Observability Logs</h1>
            </div>
            <p className="mt-0.5 hidden text-xs text-[#6e6e73] sm:block">
              Spend, latency, payloads, and provider errors from gateway traffic
            </p>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              className="h-8 border-[#d7d7dc] bg-white text-xs"
              onClick={() => load(true)}
            >
              <RefreshCw className={`size-3.5 ${refreshing ? "animate-spin" : ""}`} />
              Refresh
            </Button>
            <ThemeToggle />
          </div>
        </header>

        <main className="grid min-h-0 flex-1 grid-cols-1 overflow-hidden xl:grid-cols-[520px_1fr]">
          <section className="flex min-h-0 flex-col border-r border-[#d7d7dc] bg-white">
            <div className="border-b border-[#e5e5ea] px-4 py-3">
              <div className="grid grid-cols-3 divide-x divide-[#e5e5ea] rounded-md border border-[#d7d7dc] bg-[#fbfbfd]">
                <Metric icon={DollarSign} label="Cost" value={formatCost(totals.cost)} />
                <Metric icon={BarChart3} label="Tokens" value={totals.tokens.toLocaleString()} />
                <Metric icon={AlertTriangle} label="Errors" value={String(totals.errors)} />
              </div>
              <div className="mt-3 flex gap-2">
                <div className="relative min-w-0 flex-1">
                  <Search className="pointer-events-none absolute left-3 top-2.5 size-4 text-[#86868b]" />
                  <Input
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                    placeholder="Search request ID or model"
                    className="h-9 rounded-md border-[#d7d7dc] bg-white pl-9 text-sm shadow-none"
                  />
                </div>
                <Select value={status} onValueChange={(value) => value && setStatus(value)}>
                  <SelectTrigger className="h-9 w-[120px] rounded-md border-[#d7d7dc] bg-white">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {STATUS_OPTIONS.map((item) => (
                      <SelectItem key={item} value={item}>
                        {item === "all" ? "All" : item}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            <div className="grid grid-cols-[minmax(0,1fr)_76px_92px_86px] border-b border-[#e5e5ea] bg-[#fbfbfd] px-4 py-2 text-[11px] font-medium uppercase text-[#6e6e73]">
              <div>Request</div>
              <div>Type</div>
              <div>Tokens</div>
              <div className="text-right">Spend</div>
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto">
              {error && (
                <div className="m-4 rounded-md border border-red-200 bg-red-50 p-3 text-sm text-red-700">
                  {error}
                </div>
              )}
              {loading && <div className="p-4 text-sm text-[#6e6e73]">Loading logs...</div>}
              {!loading && logs.length === 0 && !error && (
                <div className="p-4 text-sm text-[#6e6e73]">No spend logs found.</div>
              )}
              {logs.map((log) => (
                <LogRow
                  key={log.request_id}
                  log={log}
                  active={selected?.request_id === log.request_id}
                  onSelect={async () => setSelected(await getSpendLog(log.request_id))}
                />
              ))}
            </div>
          </section>

          <section className="min-h-0 overflow-y-auto bg-[#f5f5f7]">
            {selected ? (
              <LogDetail log={selected} error={selectedError} />
            ) : (
              <div className="flex h-full items-center justify-center text-sm text-[#6e6e73]">
                Select a request log.
              </div>
            )}
          </section>
        </main>
      </div>
    </div>
  );
}

function Metric({
  icon: Icon,
  label,
  value,
}: {
  icon: LucideIcon;
  label: string;
  value: string;
}) {
  return (
    <div className="min-w-0 px-3 py-2.5">
      <div className="flex items-center gap-1.5 text-[10px] font-semibold uppercase text-[#6e6e73]">
        <Icon className="size-3.5" />
        {label}
      </div>
      <div className="mt-1 truncate font-mono text-[13px] font-semibold text-[#1d1d1f]">
        {value}
      </div>
    </div>
  );
}

function LogRow({
  log,
  active,
  onSelect,
}: {
  log: SpendLog;
  active: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      className={`grid w-full grid-cols-[minmax(0,1fr)_76px_92px_86px] items-center border-b border-[#ececf1] px-4 py-3 text-left transition ${
        active ? "bg-[#eef5ff] shadow-[inset_3px_0_0_#0a84ff]" : "hover:bg-[#f8f8fa]"
      }`}
      onClick={onSelect}
    >
      <div className="min-w-0 pr-3">
        <div className="flex min-w-0 items-center gap-2">
          <StatusDot status={log.status} />
          <span className="truncate font-mono text-[13px] font-semibold text-[#1d1d1f]">
            {log.request_id}
          </span>
        </div>
        <div className="mt-1 flex min-w-0 items-center gap-2 text-xs text-[#6e6e73]">
          <span className="truncate">{log.model_group || log.model}</span>
          <span>{formatDuration(log.request_duration_ms)}</span>
          <span>{formatDate(log.start_time)}</span>
        </div>
      </div>
      <div className="text-xs font-medium text-[#53657d]">{log.call_type}</div>
      <div className="font-mono text-xs text-[#53657d]">{log.total_tokens.toLocaleString()}</div>
      <div className="truncate text-right font-mono text-xs font-semibold text-[#1d1d1f]">
        {formatCost(log.spend)}
      </div>
    </button>
  );
}

function StatusDot({ status }: { status: string | null }) {
  const ok = status !== "error";
  return (
    <span
      className={`inline-flex size-5 shrink-0 items-center justify-center rounded-full border ${
        ok ? "border-emerald-200 bg-emerald-50 text-emerald-700" : "border-red-200 bg-red-50 text-red-700"
      }`}
      title={ok ? "Success" : "Error"}
    >
      {ok ? <Check className="size-3" /> : <AlertTriangle className="size-3" />}
    </span>
  );
}

function StatusBadge({ status }: { status: string | null }) {
  const ok = status !== "error";
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-md border px-2 py-1 text-xs font-medium ${
        ok ? "border-emerald-200 bg-emerald-50 text-emerald-700" : "border-red-200 bg-red-50 text-red-700"
      }`}
    >
      {ok ? <Check className="size-3.5" /> : <AlertTriangle className="size-3.5" />}
      {ok ? "Success" : "Error"}
    </span>
  );
}

function LogDetail({ log, error }: { log: SpendLog; error: Record<string, unknown> | null }) {
  return (
    <div className="mx-auto max-w-7xl px-6 py-5">
      <div className="rounded-lg border border-[#d7d7dc] bg-white">
        <div className="border-b border-[#e5e5ea] px-5 py-4">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="min-w-0">
              <div className="flex flex-wrap items-center gap-2 text-sm text-[#6e6e73]">
                <span className="font-medium text-[#53657d]">{log.model_group || log.model}</span>
                <StatusBadge status={log.status} />
                <span className="rounded-md border border-[#e5e5ea] px-2 py-1 text-xs">
                  {log.call_type}
                </span>
              </div>
              <div className="mt-2 flex min-w-0 items-center gap-2">
                <h2 className="truncate font-mono text-[22px] font-semibold leading-tight text-[#1d1d1f]">
                  {log.request_id}
                </h2>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 text-[#6e6e73]"
                  title="Copy request ID"
                  onClick={() => navigator.clipboard?.writeText(log.request_id)}
                >
                  <Copy className="size-4" />
                </Button>
              </div>
            </div>
            <div className="grid min-w-[420px] grid-cols-4 overflow-hidden rounded-md border border-[#d7d7dc]">
              <DetailStat label="Cost" value={formatCost(log.spend)} />
              <DetailStat label="Tokens" value={log.total_tokens.toLocaleString()} />
              <DetailStat label="Latency" value={formatDuration(log.request_duration_ms)} />
              <DetailStat label="Started" value={formatDate(log.start_time)} />
            </div>
          </div>
        </div>

        <div className="grid gap-0 2xl:grid-cols-[minmax(620px,1fr)_380px]">
          <div className="min-w-0 2xl:border-r 2xl:border-[#e5e5ea]">
            {error && (
              <Panel title="Error Trace" tone="error">
                <div className="space-y-2 text-sm">
                  <ErrorField label="Type" value={String(error.error_type ?? "-")} />
                  <ErrorField label="Message" value={String(error.message ?? "-")} />
                  <CodeBlock value={String(error.trace ?? "")} tone="error" />
                </div>
              </Panel>
            )}
            <Panel title="Request">
              <CodeBlock value={prettyJson(log.messages)} />
            </Panel>
            <Panel title="Response">
              <CodeBlock value={prettyJson(log.response)} />
            </Panel>
          </div>

          <div className="min-w-0">
            <Panel title="Request Details">
              <KeyValue label="Model" value={log.model} />
              <KeyValue label="Model Group" value={log.model_group} />
              <KeyValue label="Provider" value={log.custom_llm_provider} />
              <KeyValue label="API Base" value={log.api_base} wide />
              <KeyValue label="Session" value={log.session_id} />
            </Panel>
            <Panel title="Metrics" icon={Clock3}>
              <KeyValue label="Prompt Tokens" value={log.prompt_tokens.toLocaleString()} />
              <KeyValue label="Completion Tokens" value={log.completion_tokens.toLocaleString()} />
              <KeyValue label="Cache Hit" value={log.cache_hit ?? "false"} />
              <KeyValue label="End Time" value={formatDate(log.end_time)} wide />
              <KeyValue label="Requester IP" value={log.requester_ip_address} />
            </Panel>
            <Panel title="Metadata">
              <CodeBlock value={prettyJson(log.metadata)} compact />
            </Panel>
          </div>
        </div>
      </div>
    </div>
  );
}

function DetailStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 border-r border-[#e5e5ea] px-3 py-2 last:border-r-0">
      <div className="text-[10px] font-semibold uppercase text-[#86868b]">{label}</div>
      <div className="mt-1 truncate font-mono text-[13px] font-semibold text-[#1d1d1f]">
        {value}
      </div>
    </div>
  );
}

function Panel({
  title,
  children,
  icon: Icon,
  tone,
}: {
  title: string;
  children: ReactNode;
  icon?: LucideIcon;
  tone?: "error";
}) {
  return (
    <section className="border-b border-[#e5e5ea] last:border-b-0">
      <div className="flex h-11 items-center gap-2 px-5">
        {Icon && <Icon className="size-4 text-[#6e6e73]" />}
        <h3 className="text-[13px] font-semibold text-[#1d1d1f]">{title}</h3>
        {tone === "error" && (
          <span className="ml-auto rounded-md bg-red-50 px-2 py-1 text-xs font-medium text-red-700">
            Captured on error
          </span>
        )}
      </div>
      <div className="px-5 pb-5">{children}</div>
    </section>
  );
}

function KeyValue({
  label,
  value,
  wide = false,
}: {
  label: string;
  value: string | null | undefined;
  wide?: boolean;
}) {
  return (
    <div className="grid grid-cols-[132px_minmax(0,1fr)] gap-4 border-b border-[#f0f0f3] py-2 last:border-b-0">
      <div className="text-sm text-[#6e6e73]">{label}</div>
      <div
        className={`min-w-0 text-sm font-medium text-[#1d1d1f] ${
          wide ? "break-words" : "truncate text-right"
        }`}
      >
        {value || "-"}
      </div>
    </div>
  );
}

function ErrorField({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-red-100 bg-white px-3 py-2">
      <div className="text-[11px] font-semibold uppercase text-red-700">{label}</div>
      <div className="mt-1 break-words text-sm font-medium text-[#1d1d1f]">{value}</div>
    </div>
  );
}

function CodeBlock({
  value,
  compact = false,
  tone,
}: {
  value: string;
  compact?: boolean;
  tone?: "error";
}) {
  return (
    <pre
      className={`overflow-auto rounded-md border p-4 font-mono text-xs leading-5 ${
        compact ? "max-h-[260px]" : "max-h-[540px]"
      } ${
        tone === "error"
          ? "border-red-200 bg-red-50 text-red-950"
          : "border-[#e5e5ea] bg-[#fbfbfd] text-[#1d1d1f]"
      }`}
    >
      {value}
    </pre>
  );
}
