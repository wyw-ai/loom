import { useMemo, useState } from "react";
import type { Actor, Channel, Run, RunStatus } from "@/ipc/types";
import { EmptyState } from "@/components/shared/EmptyState";
import { PageHeader } from "@/components/shared/PageComponents";
import { StopRunButton } from "@/components/shared/StopRunButton";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { displayName } from "@/lib/format-utils";
import { runStatusLabel, runStatusDotClass, runStatusAnimationName } from "@/lib/agent-utils";
import { formatTime, shortId } from "@/lib/utils";
import { Play, AlertCircle, Clock, CheckCircle2, Loader2, RefreshCw, XCircle } from "lucide-react";

export type RunsFilter = "active" | "recent" | "failed";

export interface RunsViewProps {
  runs: Run[];
  actors: Record<string, Actor>;
  channels: Channel[];
  connection: import("@/lib/types").ConnectionState;
  busy?: string | null;
  loading?: boolean;
  error?: string | null;
  onRefresh: () => void;
  onSelectRun: (run: Run) => void;
  onCancelRun: (runId: string) => Promise<void>;
}

const TERMINAL_STATUSES: RunStatus[] = ["completed", "failed", "canceled"];

function isTerminal(status: RunStatus) {
  return TERMINAL_STATUSES.includes(status);
}

function scopeLabel(run: Run, channels: Channel[]) {
  if (run.scope.kind === "channel") {
    const channel = channels.find((c) => c.id === run.scope.id);
    return channel ? `#${channel.title}` : `#${run.scope.id}`;
  }
  return `thread:${shortId(run.scope.id)}`;
}

function runRowClass(status: RunStatus) {
  switch (status) {
    case "failed":
      return "border-red-200 bg-red-50/50";
    case "canceled":
      return "border-amber-200 bg-amber-50/50";
    case "completed":
      return "border-green-200 bg-green-50/30";
    default:
      return "border-border bg-card";
  }
}

function filterRuns(runs: Run[], filter: RunsFilter) {
  switch (filter) {
    case "active":
      return runs.filter((r) => !isTerminal(r.status));
    case "failed":
      return runs.filter((r) => r.status === "failed" || r.status === "canceled");
    case "recent":
    default:
      return runs;
  }
}

export function RunsView({ runs, actors, channels, connection, busy, loading = false, error, onRefresh, onSelectRun, onCancelRun }: RunsViewProps) {
  const [filter, setFilter] = useState<RunsFilter>("active");

  const filtered = useMemo(() => filterRuns(runs, filter), [runs, filter]);

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Runs" detail={`${runs.length} total`} />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 scrollbar-thin">
        <div className="mx-auto max-w-4xl space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              {(["active", "recent", "failed"] as RunsFilter[]).map((f) => (
                <Button
                  key={f}
                  variant={filter === f ? "default" : "outline"}
                  size="sm"
                  onClick={() => setFilter(f)}
                >
                  {f === "active" && <Play size={14} className="mr-1" />}
                  {f === "recent" && <Clock size={14} className="mr-1" />}
                  {f === "failed" && <AlertCircle size={14} className="mr-1" />}
                  {f[0].toUpperCase() + f.slice(1)}
                </Button>
              ))}
            </div>
            <Button
              variant="outline"
              size="sm"
              disabled={connection !== "open" || loading}
              onClick={onRefresh}
            >
              {loading ? (
                <Loader2 size={14} className="mr-1 animate-spin" />
              ) : (
                <RefreshCw size={14} className="mr-1" />
              )}
              Refresh
            </Button>
          </div>

          {error && (
            <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm font-medium text-red-700">
              {error}
            </div>
          )}

          {loading && runs.length === 0 ? (
            <div className="flex items-center justify-center gap-2 py-10 text-sm text-muted-foreground">
              <Loader2 size={16} className="animate-spin" />
              Loading runs…
            </div>
          ) : filtered.length === 0 ? (
            <EmptyState icon={Play} text="No runs match this filter." />
          ) : (
            <div className="space-y-2">
              {filtered.map((run) => {
                const actor = actors[run.actorId];
                const terminal = isTerminal(run.status);
                const dotClass = runStatusDotClass({
                  status: run.status,
                  run,
                  isStale: false,
                  staleThresholdSec: 0,
                  isTerminal: terminal,
                  terminalExpired: false,
                });
                const animation = runStatusAnimationName({
                  status: run.status,
                  run,
                  isStale: false,
                  staleThresholdSec: 0,
                  isTerminal: terminal,
                  terminalExpired: false,
                });
                return (
                  <div
                    key={run.id}
                    className={cn(
                      "grid gap-3 rounded-md border p-4 sm:grid-cols-[1fr_auto]",
                      runRowClass(run.status),
                    )}
                  >
                    <button
                      type="button"
                      className="text-left"
                      onClick={() => onSelectRun(run)}
                    >
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-mono text-xs text-muted-foreground">{shortId(run.id)}</span>
                        <Badge
                          variant="outline"
                          className={cn("gap-1.5 font-semibold", dotClass.replace("bg-", "text-"))}
                        >
                          <span
                            className={cn("h-2 w-2 rounded-full", dotClass)}
                            style={animation ? { animationName: animation, animationDuration: "2.2s", animationIterationCount: "infinite", animationTimingFunction: "ease-in-out" } : undefined}
                          />
                          {runStatusLabel[run.status] ?? run.status}
                        </Badge>
                        <span className="text-sm text-muted-foreground">{scopeLabel(run, channels)}</span>
                      </div>
                      <div className="mt-1 truncate font-medium">
                        {actor ? displayName(actor) : run.actorId}
                      </div>
                      <div className="text-sm text-muted-foreground">
                        {run.startReason || "No start reason"}
                      </div>
                      <div className="mt-1 text-xs text-muted-foreground">
                        Opened {formatTime(run.openedAt)}
                        {run.closedAt && ` · Closed ${formatTime(run.closedAt)}`}
                      </div>
                    </button>
                    <div className="flex items-center justify-end gap-2">
                      {!terminal && (
                        <StopRunButton
                          label="Stop"
                          busy={busy === `run:cancel:${run.id}`}
                          disabled={connection !== "open"}
                          onConfirm={() => void onCancelRun(run.id)}
                        />
                      )}
                      {terminal && (
                        <span className="flex items-center text-xs text-muted-foreground">
                          {run.status === "completed" ? (
                            <>
                              <CheckCircle2 size={14} className="mr-1 text-green-600" /> Done
                            </>
                          ) : (
                            <>
                              <XCircle size={14} className="mr-1 text-red-600" /> {run.status}
                            </>
                          )}
                        </span>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
