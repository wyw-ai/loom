import { useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import type { Actor, Run, ScopeRef } from "@/ipc/types";
import type { AgentActivityAgent, AgentActivityStatus } from "@/hooks/useAgentActivity";
import { useAgentActivity } from "@/hooks/useAgentActivity";
import { displayName } from "@/lib/format-utils";
import { runStatusLabel } from "@/lib/agent-utils";
import { cn, formatTime } from "@/lib/utils";

interface AgentActivityBannerProps {
  actors: Record<string, Actor>;
  agentActorIds: string[];
  runs: Record<string, Run>;
  scope: ScopeRef | null;
  enabled?: boolean;
}

function plural(value: number, singular: string, pluralValue = `${singular}s`) {
  return value === 1 ? singular : pluralValue;
}

function dotClass(status: AgentActivityStatus) {
  switch (status) {
    case "running":
      return "bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.45)] animate-pulse";
    case "queued":
      return "bg-amber-400";
    case "offline":
      return "bg-[#98a2b3]";
    case "unknown":
      return "bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.35)]";
    case "idle":
      return "bg-[#98a2b3]";
  }
}

function elapsedLabel(value?: string | null) {
  if (!value) return "";
  const opened = new Date(value).getTime();
  if (Number.isNaN(opened)) return "";
  const seconds = Math.max(0, Math.floor((Date.now() - opened) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return remainder > 0 ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

function statusText(agent: AgentActivityAgent) {
  switch (agent.status) {
    case "running": {
      const runLabel = agent.activeRun
        ? runStatusLabel[agent.activeRun.status] ?? agent.activeRun.status
        : "Running";
      const elapsed = elapsedLabel(agent.activeRun?.openedAt);
      return elapsed ? `${runLabel} · ${elapsed}` : runLabel;
    }
    case "queued":
      return `${agent.pending} ${plural(agent.pending, "message")} queued`;
    case "offline":
      return `Offline · ${agent.pending} ${plural(agent.pending, "message")} queued`;
    case "unknown":
      return "Status unknown · worker offline";
    case "idle":
      return "Idle";
  }
}

function ActionButton({
  children,
  disabled,
  onClick,
  title,
}: {
  children: ReactNode;
  disabled?: boolean;
  onClick: () => void;
  title?: string;
}) {
  return (
    <button
      type="button"
      title={title}
      disabled={disabled}
      onClick={onClick}
      className="rounded-md border border-[#d8deea] bg-white px-2 py-1 text-[11px] font-semibold text-[#485063] transition-colors hover:bg-[#f7f8fb] disabled:pointer-events-none disabled:opacity-50"
    >
      {children}
    </button>
  );
}

export function AgentActivityBanner({
  actors,
  agentActorIds,
  runs,
  scope,
  enabled = true,
}: AgentActivityBannerProps) {
  const [open, setOpen] = useState(false);
  const activity = useAgentActivity({
    actors,
    agentActorIds,
    runs,
    scope,
    enabled,
  });

  if (!activity.hasActivity) return null;

  const summary = `${activity.runningCount} ${plural(activity.runningCount, "agent")} running · ${activity.pendingTotal} ${plural(activity.pendingTotal, "message")} queued`;
  const actionDisabled = Boolean(activity.busyAction) || !enabled;

  if (!open) {
    return (
      <div className="border-t border-[#e2e6ef] bg-white px-5 py-1">
        <button
          type="button"
          className="mx-auto flex h-8 w-full max-w-4xl items-center gap-2 rounded-lg border border-[#e2e6ef] bg-[#fbfcff] px-3 text-xs font-semibold text-[#485063] transition-colors hover:border-[#cfd6e4] hover:bg-[#f7f8fb]"
          aria-expanded={false}
          onClick={() => setOpen(true)}
        >
          <ChevronRight size={14} className="shrink-0 text-[#667085]" />
          <span className="shrink-0">⚙</span>
          <span className="truncate">{summary}</span>
        </button>
      </div>
    );
  }

  return (
    <div className="border-t border-[#e2e6ef] bg-white px-5 py-2">
      <div className="mx-auto max-w-4xl rounded-xl border border-[#e2e6ef] bg-[#fbfcff] shadow-sm">
        <button
          type="button"
          className="flex min-h-9 w-full items-center gap-2 border-b border-[#edf0f5] px-3 py-2 text-left text-xs font-bold text-[#303849]"
          aria-expanded
          onClick={() => setOpen(false)}
        >
          <ChevronDown size={14} className="shrink-0 text-[#667085]" />
          <span className="min-w-0 flex-1 truncate">Agent activity</span>
          <span className="shrink-0 font-semibold text-[#667085]">{summary}</span>
        </button>
        {activity.error && (
          <div className="border-b border-red-100 bg-red-50 px-3 py-1.5 text-xs font-medium text-red-700">
            {activity.error}
          </div>
        )}
        <div className="divide-y divide-[#edf0f5]">
          {activity.visibleAgents.map((agent) => (
            <div key={agent.actorId} className="px-3 py-2">
              <div className="flex min-w-0 items-center gap-2">
                <span
                  className={cn("h-2.5 w-2.5 shrink-0 rounded-full", dotClass(agent.status))}
                  aria-hidden
                />
                <div className="min-w-0 flex-1">
                  <div className="flex min-w-0 items-baseline gap-2">
                    <span className="truncate text-sm font-bold text-[#303849]">
                      {displayName(agent.actor)}
                    </span>
                    <span className="shrink-0 text-xs font-semibold text-[#667085]">
                      {statusText(agent)}
                    </span>
                    {agent.status === "running" && agent.pending > 0 && (
                      <span className="shrink-0 text-xs font-semibold text-[#8a93a5]">
                        · {agent.pending} queued
                      </span>
                    )}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  {agent.activeRun && (
                    <ActionButton
                      disabled={actionDisabled}
                      title={agent.status === "unknown" ? "Force stop run" : "Stop run"}
                      onClick={() => void activity.stopRun(agent.activeRun!.id)}
                    >
                      {agent.status === "unknown" ? "Force stop" : "Stop"}
                    </ActionButton>
                  )}
                  {agent.pending > 0 && (
                    <>
                      <ActionButton
                        disabled={actionDisabled}
                        onClick={() => void activity.cancelAll(agent.actorId)}
                      >
                        Cancel all
                      </ActionButton>
                      <ActionButton
                        disabled={actionDisabled || agent.queueLoading}
                        onClick={() => activity.toggleAgentQueue(agent.actorId)}
                      >
                        {agent.expanded ? "Collapse" : "Expand"}
                      </ActionButton>
                    </>
                  )}
                </div>
              </div>
              {agent.expanded && (
                <div className="mt-2 space-y-1 rounded-lg border border-[#edf0f5] bg-white p-2">
                  {agent.queueLoading ? (
                    <div className="px-2 py-1 text-xs font-medium text-[#667085]">
                      Loading queued messages…
                    </div>
                  ) : agent.queueError ? (
                    <div className="px-2 py-1 text-xs font-medium text-red-600">
                      {agent.queueError}
                    </div>
                  ) : agent.queue.length === 0 ? (
                    <div className="px-2 py-1 text-xs font-medium text-[#667085]">
                      No queued messages.
                    </div>
                  ) : (
                    agent.queue.map((item) => (
                      <div
                        key={item.sourceId}
                        className="grid min-h-8 grid-cols-[4.5rem_minmax(6rem,8rem)_minmax(0,1fr)_auto] items-center gap-2 rounded-md px-2 py-1 text-xs text-[#485063] hover:bg-[#f7f8fb]"
                      >
                        <span className="font-medium text-[#8a93a5]">
                          {formatTime(item.updatedAt) || "—"}
                        </span>
                        <span className="truncate font-semibold text-[#303849]">
                          {item.authorName}
                        </span>
                        <span className="truncate text-[#667085]">{item.preview}</span>
                        <span className="flex shrink-0 items-center gap-1.5">
                          <ActionButton
                            disabled={actionDisabled}
                            onClick={() => void activity.expedite(agent.actorId, item.sourceId)}
                          >
                            Send now
                          </ActionButton>
                          <ActionButton
                            disabled={actionDisabled}
                            onClick={() => void activity.cancelOne(agent.actorId, item.sourceId)}
                          >
                            Cancel
                          </ActionButton>
                        </span>
                      </div>
                    ))
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
