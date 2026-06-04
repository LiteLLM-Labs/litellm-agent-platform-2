"use client";

import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import {
  AlertTriangle,
  BarChart3,
  Check,
  Copy,
  PanelRightClose,
  PanelRightOpen,
  RefreshCw,
} from "lucide-react";

import { Sidebar } from "@/components/sidebar";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import { getSpendLog, listSpendLogs } from "@/lib/api";
import type { SpendLog } from "@/lib/types";

const PAGE_SIZE = 50;
const TABLE_COLUMNS =
  "grid-cols-[150px_96px_104px_136px_190px_104px_108px_92px_132px_150px_132px_180px_132px]";

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

function shortValue(value: string | null | undefined, size = 14): string {
  if (!value) return "-";
  return value.length > size ? `${value.slice(0, size)}...` : value;
}

function metadataString(log: SpendLog | null, key: string): string | null {
  const value = log?.metadata?.[key];
  return typeof value === "string" && value.trim() ? value : null;
}

function isStreaming(log: SpendLog): boolean {
  if (!log.messages || typeof log.messages !== "object" || Array.isArray(log.messages)) {
    return false;
  }
  return (log.messages as Record<string, unknown>).stream === true;
}

function typeLabel(log: SpendLog): string {
  return log.call_type === "messages" ? "LLM" : log.call_type;
}

function timeToFirstToken(log: SpendLog): string {
  return isStreaming(log) ? formatDuration(log.request_duration_ms) : "-";
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
  const [detailOpen, setDetailOpen] = useState(true);
  const [liveTail, setLiveTail] = useState(true);
  const [page, setPage] = useState(1);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = async (silent = false) => {
    if (silent) setRefreshing(true);
    else setLoading(true);
    try {
      const next = await listSpendLogs({ limit: 250 });
      setLogs(next);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  useEffect(() => {
    if (!liveTail) return undefined;
    const timer = setInterval(() => load(true), 15_000);
    return () => clearInterval(timer);
  }, [liveTail]);

  useEffect(() => {
    setPage(1);
  }, [logs.length]);

  const totalPages = Math.max(1, Math.ceil(logs.length / PAGE_SIZE));
  const currentPage = Math.min(page, totalPages);
  const pageStart = logs.length === 0 ? 0 : (currentPage - 1) * PAGE_SIZE + 1;
  const pageEnd = Math.min(currentPage * PAGE_SIZE, logs.length);
  const visibleLogs = logs.slice(pageStart === 0 ? 0 : pageStart - 1, pageEnd);

  useEffect(() => {
    if (logs.length === 0) {
      setSelected(null);
      setDetailOpen(false);
      return;
    }
    if (selected && logs.some((log) => log.request_id === selected.request_id)) {
      return;
    }
    let cancelled = false;
    getSpendLog(logs[0].request_id)
      .then((log) => {
        if (!cancelled) {
          setSelected(log);
          setDetailOpen(true);
        }
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [logs, selected]);

  const selectedError = errorInfo(selected);

  return (
    <div className="flex h-screen bg-[#f5f5f7] text-[#1d1d1f]">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-[#d7d7dc] bg-white px-5">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <BarChart3 className="size-4 text-[#53657d]" />
              <h1 className="truncate text-[15px] font-semibold">Request Logs</h1>
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
              Fetch
            </Button>
            <ThemeToggle />
          </div>
        </header>

        <main className="relative min-h-0 flex-1 overflow-hidden">
          <section className="flex h-full min-h-0 min-w-0 flex-col bg-white">
            <div className="border-b border-[#e5e5ea] px-4 py-3">
              <div className="flex flex-wrap items-center justify-end gap-4 text-sm text-[#53657d]">
                <span>Showing {pageStart} - {pageEnd} of {logs.length} results</span>
                <span>Page {currentPage} of {totalPages}</span>
                <Button
                  variant="outline"
                  className="h-8 border-[#d7d7dc] bg-white text-sm"
                  disabled={currentPage <= 1}
                  onClick={() => setPage((value) => Math.max(1, value - 1))}
                >
                  Previous
                </Button>
                <Button
                  variant="outline"
                  className="h-8 border-[#d7d7dc] bg-white text-sm"
                  disabled={currentPage >= totalPages}
                  onClick={() => setPage((value) => Math.min(totalPages, value + 1))}
                >
                  Next
                </Button>
              </div>
            </div>

            <div className={`border-b px-4 py-2 text-sm font-medium ${
              liveTail ? "border-emerald-200 bg-emerald-50 text-emerald-700" : "border-[#e5e5ea] bg-[#fbfbfd] text-[#6e6e73]"
            }`}>
              {liveTail ? "Auto-refreshing every 15 seconds" : "Live tail paused"}
              <button className="float-right" onClick={() => setLiveTail((value) => !value)}>
                {liveTail ? "Stop" : "Start"}
              </button>
            </div>

            <div className="min-h-0 flex-1 overflow-auto">
              {error && (
                <div className="m-4 rounded-md border border-red-200 bg-red-50 p-3 text-sm text-red-700">
                  {error}
                </div>
              )}
              {loading && <div className="p-4 text-sm text-[#6e6e73]">Loading logs...</div>}
              {!loading && logs.length === 0 && !error && (
                <div className="p-4 text-sm text-[#6e6e73]">No spend logs found.</div>
              )}
              <div className="min-w-[1660px]">
                <TableHeader />
                {visibleLogs.map((log) => (
                  <LogRow
                    key={log.request_id}
                    log={log}
                    active={selected?.request_id === log.request_id}
                    onSelect={async () => {
                      setSelected(await getSpendLog(log.request_id));
                      setDetailOpen(true);
                    }}
                  />
                ))}
              </div>
            </div>
          </section>

          {selected && !detailOpen && (
            <button
              type="button"
              className="absolute right-3 top-1/2 z-20 flex -translate-y-1/2 items-center gap-2 rounded-md border border-[#c7c7cc] bg-white px-3 py-2 text-sm font-medium text-[#1d1d1f] shadow-lg"
              title="Open request details"
              onClick={() => setDetailOpen(true)}
            >
              <PanelRightOpen className="size-4 text-[#53657d]" />
              Details
            </button>
          )}

          {selected && (
            <aside
              className={`absolute inset-y-0 right-0 z-30 w-[min(760px,calc(100vw-320px))] min-w-[520px] overflow-y-auto border-l border-[#c7c7cc] bg-[#f5f5f7] shadow-[-18px_0_45px_rgba(15,23,42,0.12)] transition-transform duration-200 ease-out ${
                detailOpen ? "translate-x-0" : "pointer-events-none translate-x-full"
              }`}
            >
              <LogDetail log={selected} error={selectedError} onClose={() => setDetailOpen(false)} />
            </aside>
          )}
        </main>
      </div>
    </div>
  );
}

function TableHeader() {
  return (
    <div className={`grid ${TABLE_COLUMNS} border-b border-[#e5e5ea] bg-white px-4 py-2.5 text-[12px] font-semibold text-[#1d1d1f]`}>
      <div>Time</div>
      <div>Type</div>
      <div>Status</div>
      <div>Session ID</div>
      <div>Request ID</div>
      <div>Cost</div>
      <div>Duration (s)</div>
      <div>TTFT (s)</div>
      <div>Team Name</div>
      <div>Key Hash</div>
      <div>Key Name</div>
      <div>Model</div>
      <div>Tokens</div>
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
      className={`grid w-full ${TABLE_COLUMNS} items-center border-b border-[#ececf1] px-4 py-2.5 text-left text-[13px] transition ${
        active ? "bg-[#eef5ff] shadow-[inset_3px_0_0_#0a84ff]" : "hover:bg-[#f8f8fa]"
      }`}
      onClick={onSelect}
    >
      <div className="font-mono text-[#53657d]">{formatDate(log.start_time)}</div>
      <div><TypePill value={typeLabel(log)} /></div>
      <div><StatusBadge status={log.status} compact /></div>
      <div className="truncate font-mono text-[#0a84ff]">{shortValue(log.session_id, 13)}</div>
      <div className="truncate font-mono text-[#53657d]">{shortValue(log.request_id, 18)}</div>
      <div className="font-mono text-[#53657d]">{log.status === "error" ? "-" : formatCost(log.spend)}</div>
      <div className="font-mono text-[#53657d]">{((log.request_duration_ms ?? 0) / 1000).toFixed(2)}</div>
      <div className="font-mono text-[#53657d]">{timeToFirstToken(log)}</div>
      <div className="truncate text-[#53657d]">{metadataString(log, "team_name") ?? "-"}</div>
      <div className="truncate font-mono text-[#53657d]">{shortValue(log.api_key, 14)}</div>
      <div className="truncate text-[#53657d]">{log.user || "-"}</div>
      <div className="truncate text-[#53657d]">{log.model_group || log.model}</div>
      <div className="font-mono text-[#53657d]">
        {log.total_tokens.toLocaleString()}{" "}
        <span className="text-[#86868b]">
          ({log.prompt_tokens}+{log.completion_tokens})
        </span>
      </div>
    </button>
  );
}

function TypePill({ value }: { value: string }) {
  return (
    <span className="inline-flex items-center rounded-full border border-blue-200 bg-blue-50 px-2 py-1 text-xs font-semibold text-blue-700">
      {value}
    </span>
  );
}

function StatusBadge({ status, compact = false }: { status: string | null; compact?: boolean }) {
  const ok = status !== "error";
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-md border px-2 py-1 text-xs font-semibold ${
        ok ? "border-emerald-200 bg-emerald-50 text-emerald-700" : "border-red-200 bg-red-50 text-red-700"
      }`}
    >
      {!compact && (ok ? <Check className="size-3.5" /> : <AlertTriangle className="size-3.5" />)}
      {ok ? "Success" : "Failure"}
    </span>
  );
}

function LogDetail({
  log,
  error,
  onClose,
}: {
  log: SpendLog;
  error: Record<string, unknown> | null;
  onClose: () => void;
}) {
  return (
    <div className="space-y-5 px-6 py-5">
      <div className="border-b border-[#d7d7dc] pb-4">
        <div className="space-y-4">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <span className="text-[15px] font-semibold text-[#1d1d1f]">{log.model_group || log.model}</span>
              <span className="text-sm text-[#86868b]">{log.custom_llm_provider || "-"}</span>
              <Button
                variant="ghost"
                size="icon"
                className="ml-auto h-8 w-8 text-[#53657d]"
                title="Collapse request details"
                onClick={onClose}
              >
                <PanelRightClose className="size-4" />
              </Button>
            </div>
            <div className="mt-4 flex min-w-0 items-center gap-2">
              <h2 className="truncate font-mono text-[22px] font-semibold leading-tight text-[#1d1d1f]">
                {log.request_id}
              </h2>
              <Button
                variant="ghost"
                size="icon"
                className="h-8 w-8 text-[#0a84ff]"
                title="Copy request ID"
                onClick={() => navigator.clipboard?.writeText(log.request_id)}
              >
                <Copy className="size-4" />
              </Button>
            </div>
            <div className="mt-4 flex flex-wrap items-center gap-3">
              <StatusBadge status={log.status} />
              <span className="rounded-md border border-[#d7d7dc] bg-white px-3 py-1 text-sm">
                Env: {metadataString(log, "environment") ?? "default"}
              </span>
              <span className="text-sm text-[#86868b]">{formatDate(log.start_time)}</span>
            </div>
          </div>
          <div className="grid overflow-hidden rounded-md border border-[#d7d7dc] bg-white sm:grid-cols-4">
            <DetailStat label="Cost" value={formatCost(log.spend)} />
            <DetailStat label="Tokens" value={log.total_tokens.toLocaleString()} />
            <DetailStat label="Latency" value={formatDuration(log.request_duration_ms)} />
            <DetailStat label="TTFT" value={timeToFirstToken(log)} />
          </div>
        </div>
      </div>

      <InfoCard title="Tags">
        <TagList log={log} />
      </InfoCard>

      <InfoCard title="Request Details">
        <TwoColumnFields
          left={[
            ["Model", log.model],
            ["Call Type", log.call_type],
            ["API Base", log.api_base],
          ]}
          right={[
            ["Provider", log.custom_llm_provider],
            ["Model ID", log.model_id],
            ["IP Address", log.requester_ip_address],
          ]}
        />
      </InfoCard>

      <InfoCard title="Metrics">
        <TwoColumnFields
          left={[
            ["Input Tokens", log.prompt_tokens.toLocaleString()],
            ["Cost", formatCost(log.spend)],
            ["Time to First Token", timeToFirstToken(log)],
            ["Cache Read Tokens", metadataString(log, "cache_read_tokens") ?? "-"],
            ["Retries", "None"],
            ["End Time", log.end_time],
          ]}
          right={[
            ["Output Tokens", log.completion_tokens.toLocaleString()],
            ["Duration", formatDuration(log.request_duration_ms)],
            ["Cache Hit", log.cache_hit ?? "false"],
            ["Cache Creation Tokens", metadataString(log, "cache_creation_tokens") ?? "-"],
            ["Start Time", log.start_time],
          ]}
        />
      </InfoCard>

      {error && (
        <InfoCard title="Error Trace" tone="error">
          <div className="space-y-3">
            <ErrorField label="Type" value={String(error.error_type ?? "-")} />
            <ErrorField label="Message" value={String(error.message ?? "-")} />
            <CodeBlock value={String(error.trace ?? "")} tone="error" />
          </div>
        </InfoCard>
      )}

      <InfoCard title="Cost Breakdown">
        <div className="flex items-center justify-between text-sm">
          <span className="text-[#6e6e73]">Total</span>
          <span className="font-mono text-base font-semibold text-[#1d1d1f]">{formatCost(log.spend)}</span>
        </div>
      </InfoCard>

      <InfoCard title="Request & Response">
        <div className="space-y-3">
          <PayloadBlock title="Input" tokens={log.prompt_tokens} value={prettyJson(log.messages)} />
          <PayloadBlock title="Output" tokens={log.completion_tokens} value={prettyJson(log.response)} />
        </div>
      </InfoCard>
    </div>
  );
}

function TagList({ log }: { log: SpendLog }) {
  const rawTags = Array.isArray(log.request_tags) ? log.request_tags : [];
  const tags = rawTags.length > 0 ? rawTags : [`call_type: ${log.call_type}`, `provider: ${log.custom_llm_provider ?? "-"}`];
  return (
    <div className="flex flex-wrap gap-2">
      {tags.map((tag, index) => (
        <span
          key={`${String(tag)}-${index}`}
          className="rounded-md border border-[#d7d7dc] bg-[#fbfbfd] px-2 py-1 text-xs font-medium text-[#53657d]"
        >
          {index}: {String(tag)}
        </span>
      ))}
    </div>
  );
}

function PayloadBlock({
  title,
  tokens,
  value,
}: {
  title: string;
  tokens: number;
  value: string;
}) {
  return (
    <div className="overflow-hidden rounded-md border border-[#e5e5ea] bg-white">
      <div className="flex items-center gap-3 border-b border-[#e5e5ea] bg-[#fbfbfd] px-3 py-2 text-sm">
        <span className="font-semibold text-[#1d1d1f]">{title}</span>
        <span className="text-[#86868b]">Tokens: {tokens.toLocaleString()}</span>
        <Button
          variant="ghost"
          size="icon"
          className="ml-auto h-7 w-7 text-[#6e6e73]"
          title={`Copy ${title.toLowerCase()}`}
          onClick={() => navigator.clipboard?.writeText(value)}
        >
          <Copy className="size-3.5" />
        </Button>
      </div>
      <CodeBlock value={value} />
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

function InfoCard({
  title,
  children,
  tone,
}: {
  title: string;
  children: ReactNode;
  tone?: "error";
}) {
  return (
    <section className={`overflow-hidden rounded-lg border bg-white shadow-sm ${
      tone === "error" ? "border-red-200" : "border-[#e5e5ea]"
    }`}>
      <div className="flex h-12 items-center gap-2 border-b border-[#e5e5ea] px-5">
        <h3 className="text-[15px] font-semibold text-[#1d1d1f]">{title}</h3>
        {tone === "error" && (
          <span className="ml-auto rounded-md bg-red-50 px-2 py-1 text-xs font-medium text-red-700">
            Captured on error
          </span>
        )}
      </div>
      <div className="p-5">{children}</div>
    </section>
  );
}

function TwoColumnFields({
  left,
  right,
}: {
  left: Array<[string, string | null | undefined]>;
  right: Array<[string, string | null | undefined]>;
}) {
  return (
    <div className="grid gap-x-14 gap-y-3 md:grid-cols-2">
      <div className="space-y-3">
        {left.map(([label, value]) => (
          <InlineMetric key={label} label={label} value={value} />
        ))}
      </div>
      <div className="space-y-3">
        {right.map(([label, value]) => (
          <InlineMetric key={label} label={label} value={value} />
        ))}
      </div>
    </div>
  );
}

function InlineMetric({ label, value }: { label: string; value: string | null | undefined }) {
  return (
    <div className="flex min-w-0 items-baseline gap-2 text-[15px]">
      <span className="shrink-0 text-[#86868b]">{label}:</span>
      <span className="min-w-0 break-words font-medium text-[#1d1d1f]">{value || "-"}</span>
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
