import { useEffect, useRef, useState } from "react";
import { ExternalLink, Loader2, Search, X } from "lucide-react";
import type { Actor, Channel, Message, Thread } from "@/ipc/types";
import type { MessageContextResult, MessageSearchParams } from "@/ipc/types";
import { channelTarget, threadTarget } from "@/ipc/types";
import type { ConnectionState } from "@/lib/types";
import { cn, formatTime, shortId } from "@/lib/utils";
import { displayName } from "@/lib/format-utils";
import * as ipc from "@/ipc/bridge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { EmptyState } from "@/components/shared/EmptyState";
import { useI18n } from "@/lib/i18n";

type TimeChip = "all" | "today" | "7d" | "30d" | "custom";
type ScopeChoice = "current" | "all";

const SEARCH_LIMIT = 20;
const CONTEXT_WINDOW = 10;

const TIME_CHIPS: Array<{ id: TimeChip; label: string }> = [
  { id: "all", label: "All time" },
  { id: "today", label: "Today" },
  { id: "7d", label: "7 days" },
  { id: "30d", label: "30 days" },
  { id: "custom", label: "Custom" },
];

function startOfLocalDay(date: Date): Date {
  const result = new Date(date);
  result.setHours(0, 0, 0, 0);
  return result;
}

/** YYYY-MM-DD from a date input → that local calendar day at 00:00. */
function localDateBoundary(value: string): Date | null {
  if (!value) return null;
  const [year, month, day] = value.split("-").map(Number);
  if (!year || !month || !day) return null;
  const date = new Date(year, month - 1, day, 0, 0, 0, 0);
  if (
    date.getFullYear() !== year ||
    date.getMonth() !== month - 1 ||
    date.getDate() !== day
  ) {
    return null;
  }
  return date;
}

function addLocalDays(date: Date, days: number): Date {
  const result = new Date(date);
  result.setDate(result.getDate() + days);
  return result;
}

/** Local date boundaries → absolute RFC3339 UTC strings in the user's timezone. */
function timeRange(
  chip: TimeChip,
  customAfter: string,
  customBefore: string,
): { createdAfter?: string; createdBefore?: string } {
  const now = new Date();
  const today = startOfLocalDay(now);
  const tomorrow = addLocalDays(today, 1);
  switch (chip) {
    case "today":
      return {
        createdAfter: today.toISOString(),
        createdBefore: tomorrow.toISOString(),
      };
    case "7d":
      return {
        createdAfter: addLocalDays(today, -6).toISOString(),
        createdBefore: tomorrow.toISOString(),
      };
    case "30d":
      return {
        createdAfter: addLocalDays(today, -29).toISOString(),
        createdBefore: tomorrow.toISOString(),
      };
    case "custom": {
      const after = localDateBoundary(customAfter);
      const before = localDateBoundary(customBefore);
      const exclusiveBefore = before ? addLocalDays(before, 1) : null;
      return {
        ...(after ? { createdAfter: after.toISOString() } : {}),
        // createdBefore is exclusive: next local midnight after the chosen end date.
        ...(exclusiveBefore ? { createdBefore: exclusiveBefore.toISOString() } : {}),
      };
    }
    default:
      return {};
  }
}

/** JSON-RPC -32000 (not-found) travels as a flattened string through the Tauri bridge. */
function isNotFoundError(err: unknown): boolean {
  const text = err instanceof Error ? err.message : String(err);
  return text.includes("-32000");
}

function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function SearchPanel({
  actors,
  channels,
  activeChannel,
  activeThread,
  connection,
  className,
  onClose,
  onOpenContext,
}: {
  actors: Record<string, Actor>;
  channels: Channel[];
  activeChannel: Channel | null;
  activeThread: Thread | null;
  connection: ConnectionState;
  className?: string;
  onClose: () => void;
  onOpenContext: (context: MessageContextResult) => Promise<void> | void;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [scopeChoice, setScopeChoice] = useState<ScopeChoice>("current");
  const [timeChip, setTimeChip] = useState<TimeChip>("all");
  const [customAfter, setCustomAfter] = useState("");
  const [customBefore, setCustomBefore] = useState("");
  const [results, setResults] = useState<Message[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [contextBusy, setContextBusy] = useState(false);
  const [contextError, setContextError] = useState<string | null>(null);
  const mountedRef = useRef(false);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const currentTarget = activeThread
    ? threadTarget(activeThread)
    : activeChannel
      ? channelTarget(activeChannel.id)
      : null;
  const currentScopeLabel = activeThread
    ? t("This thread")
    : activeChannel
      ? `#${activeChannel.title}`
      : t("Current channel");

  const breadcrumb = (message: Message): string => {
    if (message.scope.kind === "channel") {
      const channel = channels.find((c) => c.id === message.scope.id);
      return channel ? `#${channel.title}` : `#${message.scope.id}`;
    }
    return `thread:${shortId(message.scope.id)}`;
  };

  const authorLabel = (message: Message): string =>
    (message.authorActorId && actors[message.authorActorId]
      ? displayName(actors[message.authorActorId])
      : "") || message.authorActorId;

  const handleSearch = async () => {
    const trimmed = query.trim();
    if (!trimmed || busy || connection !== "open") return;
    const range = timeRange(timeChip, customAfter, customBefore);
    if (
      range.createdAfter &&
      range.createdBefore &&
      range.createdAfter >= range.createdBefore
    ) {
      setError(t("Start date must be on or before end date."));
      return;
    }
    setBusy(true);
    setError(null);
    setContextError(null);
    try {
      const params: MessageSearchParams = { query: trimmed, limit: SEARCH_LIMIT };
      if (scopeChoice === "current" && currentTarget) params.target = currentTarget;
      if (range.createdAfter) params.createdAfter = range.createdAfter;
      if (range.createdBefore) params.createdBefore = range.createdBefore;
      const result = await ipc.messageSearch(params);
      if (!mountedRef.current) return;
      setResults(result.messages);
    } catch (err) {
      if (!mountedRef.current) return;
      console.error("message.search failed", err);
      setResults(null);
      setError(t("Search failed: {{error}}", { error: errorText(err) }));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  };

  const handleLoadContext = async (messageId: string) => {
    if (contextBusy || connection !== "open") return;
    setContextBusy(true);
    setContextError(null);
    try {
      const result = await ipc.messageContext({
        messageId,
        before: CONTEXT_WINDOW,
        after: CONTEXT_WINDOW,
      });
      if (!mountedRef.current) return;
      await onOpenContext(result);
      onClose();
    } catch (err) {
      if (!mountedRef.current) return;
      console.error("message.context failed", err);
      setContextError(isNotFoundError(err)
        ? t("The message was deleted or is not visible.")
        : t("Context failed: {{error}}", { error: errorText(err) }));
    } finally {
      if (mountedRef.current) setContextBusy(false);
    }
  };

  return (
    <aside
      className={cn(
        "min-h-0 min-w-0 flex-col bg-white",
        className ?? "hidden border-l border-[#e2e6ef] xl:flex",
      )}
    >
      {/* Header */}
      <div className="flex min-h-[86px] shrink-0 items-center border-b border-[#e2e6ef] bg-white px-5 py-3">
        <div className="flex min-w-0 flex-1 items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="min-w-0 truncate text-lg font-bold text-[#111827]">
              {t("Search messages")}
            </div>
            <div className="mt-0.5 truncate text-sm text-[#485063]">
              {scopeChoice === "current" && currentTarget ? currentScopeLabel : t("All channels")}
            </div>
          </div>
          <button className="composer-icon" type="button" title={t("Close")} onClick={onClose}>
            <X size={16} />
          </button>
        </div>
      </div>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-5 py-4 scrollbar-thin">
        {/* Query */}
        <div className="flex items-center gap-2">
          <Input
            placeholder={t("Search messages...")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void handleSearch()}
          />
          <Button
            size="sm"
            className="h-9 shrink-0"
            disabled={busy || !query.trim() || connection !== "open"}
            onClick={() => void handleSearch()}
          >
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Search size={14} />}
            {t("Search")}
          </Button>
        </div>

        {/* Scope selector */}
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold uppercase tracking-wide text-[#596174]">{t("Scope")}</span>
          <Button
            variant={scopeChoice === "current" ? "default" : "outline"}
            size="sm"
            disabled={!currentTarget}
            onClick={() => setScopeChoice("current")}
          >
            {currentScopeLabel}
          </Button>
          <Button
            variant={scopeChoice === "all" ? "default" : "outline"}
            size="sm"
            onClick={() => setScopeChoice("all")}
          >
            {t("All")}
          </Button>
        </div>

        {/* Time filter chips */}
        <div className="flex flex-wrap items-center gap-1.5">
          {TIME_CHIPS.map((chip) => (
            <button
              key={chip.id}
              type="button"
              aria-pressed={timeChip === chip.id}
              onClick={() => setTimeChip(chip.id)}
              className={cn(
                "rounded-full border px-2.5 py-1 text-xs font-medium",
                timeChip === chip.id
                  ? "border-[#bdb7ff] bg-[#f1efff] text-[#5843d7]"
                  : "border-[#dfe3ec] bg-white text-[#485063] hover:bg-[#f7f8fb]",
              )}
            >
              {t(chip.label)}
            </button>
          ))}
        </div>
        {timeChip === "custom" && (
          <div className="grid grid-cols-2 gap-2">
            <label className="text-xs text-[#596174]">
              {t("From date")}
              <Input
                type="date"
                value={customAfter}
                max={customBefore || undefined}
                onChange={(e) => setCustomAfter(e.target.value)}
              />
            </label>
            <label className="text-xs text-[#596174]">
              {t("To date (inclusive)")}
              <Input
                type="date"
                value={customBefore}
                min={customAfter || undefined}
                onChange={(e) => setCustomBefore(e.target.value)}
              />
            </label>
          </div>
        )}

        <div className="text-xs text-[#98a2b3]">{t("At most {{count}} results are shown.", { count: SEARCH_LIMIT })}</div>
        {error && <div className="text-sm font-medium text-red-600">{error}</div>}

        {/* Results */}
        {results && results.length === 0 && !busy && (
          <EmptyState icon={Search} text={t("No matches.")} />
        )}
        {results && results.length > 0 && (
          <div className="space-y-1.5">
            {results.map((message) => (
              <div key={message.id} className="rounded-md border border-border bg-card p-2 text-sm">
                <button
                  type="button"
                  className="w-full text-left"
                  disabled={contextBusy}
                  onClick={() => void handleLoadContext(message.id)}
                >
                  <div className="flex items-center gap-2 text-xs text-muted-foreground">
                    <Badge variant="outline" className="shrink-0">{breadcrumb(message)}</Badge>
                    <span className="truncate">{authorLabel(message)}</span>
                    <span className="shrink-0">{formatTime(message.createdAt)}</span>
                  </div>
                  <div className="mt-0.5 line-clamp-2">{message.body || t("(no body)")}</div>
                </button>
                <div className="mt-1 flex justify-end">
                  <button
                    type="button"
                    className="inline-flex items-center gap-1 text-xs font-medium text-[#5843d7] hover:underline"
                    disabled={contextBusy}
                    onClick={() => void handleLoadContext(message.id)}
                  >
                    <ExternalLink size={11} />
                    {t("Open source")}
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Hit navigation */}
        {contextBusy && (
          <div className="flex items-center gap-2 text-sm text-[#667085]">
            <Loader2 size={14} className="animate-spin" />
            {t("Loading context and opening source…")}
          </div>
        )}
        {contextError && <div className="text-sm font-medium text-red-600">{contextError}</div>}
      </div>
    </aside>
  );
}
