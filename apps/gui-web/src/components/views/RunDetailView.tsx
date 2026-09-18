import { useEffect, useState } from "react";
import type { Actor, Channel, Run } from "@/ipc/types";
import * as ipc from "@/ipc/bridge";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { StopRunButton } from "@/components/shared/StopRunButton";
import { cn, formatTime, shortId } from "@/lib/utils";
import { displayName } from "@/lib/format-utils";
import { runStatusLabel, runStatusDotClass, runStatusAnimationName } from "@/lib/agent-utils";
import { ArrowLeft, ExternalLink, Loader2, RefreshCw } from "lucide-react";

export interface RunDetailViewProps {
  run: Run;
  actors: Record<string, Actor>;
  channels: Channel[];
  connection: import("@/lib/types").ConnectionState;
  cancelBusy?: boolean;
  onBack: () => void;
  onCancel: (runId: string) => Promise<void>;
  onOpenScope?: (run: Run) => void;
}

const TERMINAL_STATUSES = ["completed", "failed", "canceled"];

function isTerminal(status: Run["status"]) {
  return TERMINAL_STATUSES.includes(status);
}

function scopeLabel(run: Run, channels: Channel[]) {
  if (run.scope.kind === "channel") {
    const channel = channels.find((c) => c.id === run.scope.id);
    return channel ? `#${channel.title}` : `#${run.scope.id}`;
  }
  return `thread:${shortId(run.scope.id)}`;
}

export function RunDetailView({
  run,
  actors,
  channels,
  connection,
  cancelBusy = false,
  onBack,
  onCancel,
  onOpenScope,
}: RunDetailViewProps) {
  const [canonicalRun, setCanonicalRun] = useState(run);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [refreshVersion, setRefreshVersion] = useState(0);

  useEffect(() => {
    setCanonicalRun(run);
    setLoadError(null);
  }, [run]);

  useEffect(() => {
    if (connection !== "open") {
      setLoading(false);
      return;
    }
    let alive = true;
    setLoading(true);
    setLoadError(null);
    void ipc
      .runGet({ runId: run.id })
      .then((result) => {
        if (!alive) return;
        setCanonicalRun(result.run);
      })
      .catch((err) => {
        if (!alive) return;
        setLoadError(`Unable to refresh run: ${err instanceof Error ? err.message : String(err)}`);
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [connection, refreshVersion, run.id]);

  const displayRun = canonicalRun.id === run.id ? canonicalRun : run;
  const actor = actors[displayRun.actorId];
  const terminal = isTerminal(displayRun.status);
  const ctx = {
    status: displayRun.status,
    run: displayRun,
    isStale: false,
    staleThresholdSec: 0,
    isTerminal: terminal,
    terminalExpired: false,
  };
  const dotClass = runStatusDotClass(ctx);
  const animation = runStatusAnimationName(ctx);

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <header className="flex h-[86px] shrink-0 items-center justify-between border-b border-[#e2e6ef] bg-white px-6">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="sm" onClick={onBack}>
            <ArrowLeft size={16} className="mr-1" />
            Back
          </Button>
          <h1 className="text-[22px] font-bold text-[#111827]">{`Run ${shortId(displayRun.id)}`}</h1>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-[#667085]">
            {runStatusLabel[displayRun.status] ?? displayRun.status}
          </span>
          <Button
            variant="outline"
            size="sm"
            disabled={connection !== "open" || loading}
            onClick={() => setRefreshVersion((version) => version + 1)}
          >
            {loading ? (
              <Loader2 size={14} className="mr-1 animate-spin" />
            ) : (
              <RefreshCw size={14} className="mr-1" />
            )}
            Refresh
          </Button>
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto p-5 scrollbar-thin">
        <div className="mx-auto max-w-4xl space-y-4">
          <div className="rounded-md border border-border bg-card p-4">
            <div className="flex flex-wrap items-center gap-3">
              <Badge variant="outline" className={cn("gap-1.5 font-semibold", dotClass.replace("bg-", "text-"))}>
                <span
                  className={cn("h-2 w-2 rounded-full", dotClass)}
                  style={animation ? { animationName: animation, animationDuration: "2.2s", animationIterationCount: "infinite", animationTimingFunction: "ease-in-out" } : undefined}
                />
                {runStatusLabel[displayRun.status] ?? displayRun.status}
              </Badge>
              {!terminal && (
                <StopRunButton
                  key={displayRun.id}
                  label="Stop run"
                  busy={cancelBusy}
                  disabled={connection !== "open"}
                  onConfirm={() => void onCancel(displayRun.id)}
                />
              )}
            </div>

            <dl className="mt-4 grid gap-3 text-sm sm:grid-cols-[120px_1fr]">
              <dt className="text-muted-foreground">ID</dt>
              <dd className="font-mono">{displayRun.id}</dd>

              <dt className="text-muted-foreground">Actor</dt>
              <dd>{actor ? displayName(actor) : displayRun.actorId}</dd>

              <dt className="text-muted-foreground">Scope</dt>
              <dd className="flex items-center gap-2">
                {scopeLabel(displayRun, channels)}
                {onOpenScope && (
                  <Button variant="ghost" size="sm" onClick={() => onOpenScope(displayRun)}>
                    <ExternalLink size={14} className="mr-1" />
                    Open
                  </Button>
                )}
              </dd>

              <dt className="text-muted-foreground">Start reason</dt>
              <dd>{displayRun.startReason || "—"}</dd>

              <dt className="text-muted-foreground">Opened</dt>
              <dd>{formatTime(displayRun.openedAt)}</dd>

              {displayRun.closedAt && (
                <>
                  <dt className="text-muted-foreground">Closed</dt>
                  <dd>{formatTime(displayRun.closedAt)}</dd>
                </>
              )}

              {displayRun.deliveryId && (
                <>
                  <dt className="text-muted-foreground">Delivery</dt>
                  <dd className="font-mono">{displayRun.deliveryId}</dd>
                </>
              )}

              {displayRun.agentConfigVersionId && (
                <>
                  <dt className="text-muted-foreground">Config</dt>
                  <dd className="font-mono">{displayRun.agentConfigVersionId}</dd>
                </>
              )}
            </dl>
          </div>

          {loadError && (
            <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm font-medium text-red-700">
              {loadError}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
