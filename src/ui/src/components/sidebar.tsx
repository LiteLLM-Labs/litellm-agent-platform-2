"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import {
  Activity,
  Bot,
  FileText,
  Inbox,
  KeyRound,
  Plus,
  Puzzle,
  Settings,
  ShieldCheck,
  Trash2,
  Users,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { usePathname } from "next/navigation";
import { Button } from "@/components/ui/button";
import { readHarness } from "@/lib/use-harness";
import { createSession, deleteSession, listSessions, listInbox } from "@/lib/api";
import type { OpencodeSession } from "@/lib/types";

type NavItem = {
  label: string;
  href: string;
  icon: LucideIcon;
  active: (pathname: string) => boolean;
  badge?: number;
};

type NavSection = {
  label: string;
  items: NavItem[];
};

function timeAgo(ts?: number): string {
  if (!ts) return "";
  const secs = Math.max(0, Math.floor((Date.now() - ts) / 1000));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h`;
  return `${Math.floor(hrs / 24)}d`;
}

export function Sidebar({ activeId }: { activeId?: string | null }) {
  const router = useRouter();
  const pathname = usePathname();
  const [sessions, setSessions] = useState<OpencodeSession[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [inboxCount, setInboxCount] = useState(0);
  const load = async () => {
    try {
      const list = await listSessions();
      setSessions(list);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, []);

  // Poll the needs-attention count for the unread badge.
  useEffect(() => {
    const loadCount = () =>
      listInbox("attention")
        .then((items) => setInboxCount(items.length))
        .catch(() => {});
    loadCount();
    const t = setInterval(loadCount, 5000);
    return () => clearInterval(t);
  }, [pathname]);

  const onNew = async () => {
    setCreating(true);
    try {
      const s = await createSession(undefined, readHarness());
      router.push(`/chat/?id=${encodeURIComponent(s.id)}`);
      load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setCreating(false);
    }
  };

  const onDelete = async (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    setSessions((prev) => prev?.filter((s) => s.id !== id) ?? null);
    await deleteSession(id);
    if (id === activeId) router.push("/sessions/");
  };

  const currentPath = pathname ?? "";
  const sections: NavSection[] = [
    {
      label: "AI Gateway",
      items: [
        {
          label: "Agents",
          href: "/agents/",
          icon: Bot,
          active: (path) => path.startsWith("/agents"),
        },
        {
          label: "Inbox",
          href: "/inbox/",
          icon: Inbox,
          active: (path) => path.startsWith("/inbox"),
          badge: inboxCount,
        },
        {
          label: "Integrations",
          href: "/integrations/",
          icon: Puzzle,
          active: (path) => path.startsWith("/integrations"),
        },
        {
          label: "Skills",
          href: "/skills/",
          icon: FileText,
          active: (path) => path.startsWith("/skills"),
        },
        {
          label: "Vault",
          href: "/vault/",
          icon: KeyRound,
          active: (path) => path.startsWith("/vault"),
        },
      ],
    },
    {
      label: "Access Control",
      items: [
        {
          label: "Keys",
          href: "/keys/",
          icon: ShieldCheck,
          active: (path) => path.startsWith("/keys"),
        },
        {
          label: "Teams",
          href: "/teams/",
          icon: Users,
          active: (path) => path.startsWith("/teams"),
        },
      ],
    },
    {
      label: "Observability",
      items: [
        {
          label: "Logs",
          href: "/observability/logs/",
          icon: Activity,
          active: (path) => path.startsWith("/observability"),
        },
      ],
    },
  ];

  return (
    <aside className="flex h-screen w-16 shrink-0 flex-col border-r border-border bg-background sm:w-64">
      <div className="flex h-12 items-center justify-center border-b border-border px-2 sm:justify-between sm:px-4">
        <div
          className="flex min-w-0 cursor-pointer items-center gap-2"
          onClick={() => router.push("/sessions/")}
        >
          <span className="text-xl leading-none">🚄</span>
          <span className="hidden text-sm font-semibold sm:inline">LiteLLM</span>
        </div>
      </div>

      <div className="space-y-3 border-b border-border px-2 py-3 sm:px-3">
        <Button
          onClick={onNew}
          disabled={creating}
          className="relative w-full justify-center sm:justify-start"
          size="sm"
          aria-label="New session"
        >
          <Plus className="size-4" />
          <span className="hidden sm:inline">New session</span>
        </Button>
        <div className="space-y-3">
          {sections.map((section) => (
            <div key={section.label} className="space-y-1">
              <div className="hidden px-2 pb-1 pt-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground sm:block">
                {section.label}
              </div>
              <div className="space-y-1">
                {section.items.map((item) => {
                  const Icon = item.icon;
                  const badge = item.badge ?? 0;
                  return (
                    <Button
                      key={item.href}
                      onClick={() => router.push(item.href)}
                      variant={item.active(currentPath) ? "secondary" : "ghost"}
                      className="relative w-full justify-center sm:justify-start"
                      size="sm"
                      aria-label={item.label}
                      title={item.label}
                    >
                      <Icon className="size-4" />
                      <span className="hidden sm:inline">{item.label}</span>
                      {badge > 0 && (
                        <span className="absolute ml-7 mt-[-18px] flex h-4 min-w-4 items-center justify-center rounded-full bg-amber-500 px-1 text-[10px] font-semibold text-white sm:static sm:ml-auto sm:mt-0 sm:h-5 sm:min-w-5 sm:px-1.5 sm:text-[11px]">
                          {badge}
                        </span>
                      )}
                    </Button>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="hidden flex-1 overflow-y-auto py-2 sm:block">
        <div className="px-4 pb-1 pt-1 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
          Agent Sessions
        </div>
        {error && (
          <div className="px-3 py-2 text-xs text-destructive">{error}</div>
        )}
        {!sessions && !error && (
          <div className="px-3 py-2 text-xs text-muted-foreground">Loading…</div>
        )}
        {sessions && sessions.length === 0 && (
          <div className="px-3 py-2 text-xs text-muted-foreground">
            No sessions yet.
          </div>
        )}
        {sessions?.map((s) => {
          const short = s.id.slice(0, 12);
          const title = s.title?.trim() || short;
          const active = s.id === activeId;
          return (
            <div
              key={s.id}
              onClick={() => router.push(`/chat/?id=${encodeURIComponent(s.id)}`)}
              className={`group mx-2 px-2 py-1.5 rounded text-xs cursor-pointer flex items-center justify-between gap-2 ${
                active
                  ? "bg-accent text-accent-foreground"
                  : "hover:bg-accent/50"
              }`}
            >
              <div className="min-w-0 flex-1">
                <div className="truncate font-medium">{title}</div>
                <div className="font-mono text-[10px] text-muted-foreground truncate">
                  {(s.agent ?? s.harness) === "claude-code" ? "cc" : (s.agent ?? s.harness) === "github-copilot" ? "gh" : "oc"} · {short} · {timeAgo(s.time?.created)}
                </div>
              </div>
              <button
                onClick={(e) => onDelete(e, s.id)}
                className="opacity-0 group-hover:opacity-100 transition-opacity p-1 hover:bg-background rounded"
                aria-label="Delete session"
              >
                <Trash2 className="size-3" />
              </button>
            </div>
          );
        })}
      </div>

      <div className="border-t border-border p-2 sm:p-3">
        <Button
          onClick={() => router.push("/settings/")}
          variant={pathname?.startsWith("/settings") ? "secondary" : "ghost"}
          className="w-full justify-center sm:justify-start"
          size="sm"
          aria-label="Settings"
        >
          <Settings className="size-4" />
          <span className="hidden sm:inline">Settings</span>
        </Button>
      </div>
    </aside>
  );
}
