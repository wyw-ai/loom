import { useMemo, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import type {
  Actor,
  MachineInfo,
  Run,
  ScopeRef,
  ServiceRuntimePhase,
  ServiceRuntimeState,
} from "@/ipc/types";
import type { AgentActivityAgent, AgentActivityStatus } from "@/hooks/useAgentActivity";
import { useAgentActivity } from "@/hooks/useAgentActivity";
import { StopRunButton } from "@/components/shared/StopRunButton";
import { displayName } from "@/lib/format-utils";
import { runStatusLabel } from "@/lib/agent-utils";
import { cn, formatTime } from "@/lib/utils";
import { useI18n, type I18nContextValue } from "@/lib/i18n";

export interface ActorActivityBannerProps {
  actors: Record<string, Actor>;
  /** Actors related to the current surface. Capability-specific adapters decide
   * which lifecycle source applies to each actor kind. */
  actorIds: string[];
  machines?: MachineInfo[];
  runs: Record<string, Run>;
  scope: ScopeRef | null;
  enabled?: boolean;
}

export interface ActorActivityService {
  key: string;
  actor: Actor;
  machineId: string;
  machineName: string;
  serviceId: string;
  pluginKind: string;
  lifecycle: ServiceRuntimeState["lifecycle"];
  phase: ServiceRuntimePhase;
  instances: number;
  lastError: string | null;
}

const SERVICE_PHASE_PRIORITY: Record<ServiceRuntimePhase, number> = {
  failed: 3,
  starting: 2,
  running: 1,
};

function runtimeMatchesScope(runtime: ServiceRuntimeState, scope: ScopeRef | null) {
  if (!scope) return false;
  return runtime.scopes.some(
    (candidate) => candidate.kind === scope.kind && candidate.id === scope.id,
  );
}

/** Build scope-local service rows without treating an actor connection as runtime activity. */
export function scopedServiceActivity(
  machines: MachineInfo[],
  actors: Record<string, Actor>,
  scope: ScopeRef | null,
): ActorActivityService[] {
  const grouped = new Map<string, {
    machine: MachineInfo;
    runtimes: Map<string, ServiceRuntimeState>;
  }>();

  for (const ownerMachine of machines) {
    for (const runtime of ownerMachine.serviceRuntimeStates ?? []) {
      if (!runtimeMatchesScope(runtime, scope)) continue;
      const machine = machines.find((item) => item.id === runtime.machineId) ?? ownerMachine;
      const key = `${runtime.machineId}:${runtime.serviceId}`;
      const group = grouped.get(key) ?? { machine, runtimes: new Map() };
      group.runtimes.set(runtime.runtimeId, runtime);
      grouped.set(key, group);
    }
  }

  return Array.from(grouped, ([key, group]) => {
    const runtimes = Array.from(group.runtimes.values());
    const representative = runtimes.reduce((best, runtime) =>
      SERVICE_PHASE_PRIORITY[runtime.phase] > SERVICE_PHASE_PRIORITY[best.phase]
        ? runtime
        : best,
    );
    const spec = group.machine.services.find((service) => service.id === representative.serviceId);
    const actor = actors[representative.actorId] ?? spec?.actor ?? {
      id: representative.actorId,
      kind: "service" as const,
      displayName: spec?.displayName || representative.serviceId,
    };
    const failedRuntime = runtimes.find((runtime) => runtime.phase === "failed" && runtime.lastError);
    return {
      key,
      actor,
      machineId: representative.machineId,
      machineName: group.machine.name || representative.machineId,
      serviceId: representative.serviceId,
      pluginKind: representative.pluginKind,
      lifecycle: representative.lifecycle,
      phase: representative.phase,
      instances: runtimes.length,
      lastError: failedRuntime?.lastError ?? null,
    };
  }).sort((left, right) => {
    const phaseDifference = SERVICE_PHASE_PRIORITY[right.phase] - SERVICE_PHASE_PRIORITY[left.phase];
    if (phaseDifference !== 0) return phaseDifference;
    return displayName(left.actor).localeCompare(displayName(right.actor));
  });
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

function statusText(agent: AgentActivityAgent, t: I18nContextValue["t"]) {
  switch (agent.status) {
    case "running": {
      const runLabel = agent.activeRun
        ? runStatusLabel[agent.activeRun.status] ?? agent.activeRun.status
        : "Running";
      const elapsed = elapsedLabel(agent.activeRun?.openedAt);
      return elapsed
        ? t("{{status}} · {{elapsed}}", { status: t(runLabel), elapsed })
        : t(runLabel);
    }
    case "queued":
      return t("{{count}} messages queued", { count: agent.pending });
    case "offline":
      return t("Offline · {{count}} messages queued", { count: agent.pending });
    case "unknown":
      return t("Status unknown · worker offline");
    case "idle":
      return t("Idle");
  }
}

function serviceDotClass(phase: ServiceRuntimePhase) {
  switch (phase) {
    case "running":
      return "bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.45)] animate-pulse";
    case "starting":
      return "bg-amber-400 shadow-[0_0_8px_rgba(251,191,36,0.35)] animate-pulse";
    case "failed":
      return "bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.35)]";
  }
}

function servicePhaseText(phase: ServiceRuntimePhase, t: I18nContextValue["t"]) {
  switch (phase) {
    case "starting":
      return t("Starting");
    case "running":
      return t("Running");
    case "failed":
      return t("Failed");
  }
}

function serviceLifecycleText(
  lifecycle: ServiceRuntimeState["lifecycle"],
  t: I18nContextValue["t"],
) {
  return lifecycle === "thread_bound" ? t("Thread bound") : t("Channel singleton");
}

function serviceInstanceText(count: number, t: I18nContextValue["t"]) {
  return count === 1 ? t("1 instance") : t("{{count}} instances", { count });
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

export function ActorActivityBanner({
  actors,
  actorIds,
  machines = [],
  runs,
  scope,
  enabled = true,
}: ActorActivityBannerProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const agentActorIds = useMemo(
    () => actorIds.filter((actorId) => actors[actorId]?.kind === "agent"),
    [actorIds, actors],
  );
  const activity = useAgentActivity({
    actors,
    agentActorIds,
    runs,
    scope,
    enabled,
  });
  const services = useMemo(
    () => scopedServiceActivity(machines, actors, scope),
    [actors, machines, scope],
  );

  if (!activity.hasActivity && services.length === 0) return null;

  const primaryAgent = activity.primaryAgent ?? null;
  const primaryService = primaryAgent ? null : services[0] ?? null;
  const fallbackAgent = primaryAgent || primaryService ? null : activity.visibleAgents[0] ?? null;
  const compactAgent = primaryAgent ?? fallbackAgent;
  const totalActorCount = activity.visibleAgents.length + services.length;
  const otherActorCount = Math.max(0, totalActorCount - 1);
  const activeActorCount = activity.activeCount + services.filter(
    (service) => service.phase !== "failed",
  ).length;
  const failedServiceCount = services.filter((service) => service.phase === "failed").length;
  const summaryParts = [
    activeActorCount > 0
      ? t("{{count}} actors active", { count: activeActorCount })
      : null,
    failedServiceCount > 0
      ? t("{{count}} actors need attention", { count: failedServiceCount })
      : null,
    activity.pendingTotal > 0
      ? t("{{count}} messages queued", { count: activity.pendingTotal })
      : null,
    activeActorCount === 0 && failedServiceCount === 0 && activity.pendingTotal === 0
      ? t("{{count}} actors offline", { count: activity.visibleAgents.length })
      : null,
  ].filter(Boolean);
  const summary = summaryParts.join(" · ");
  const actionDisabled = Boolean(activity.busyAction) || !enabled;

  if (!open) {
    return (
      <div className="actor-activity-banner shrink-0 border-t border-[#e2e6ef] bg-white px-5 py-1">
        <div className="mx-auto flex h-9 w-full max-w-4xl items-center rounded-lg border border-[#e2e6ef] bg-[#fbfcff] pr-2 text-xs text-[#485063] shadow-sm">
          <button
            type="button"
            className="flex h-full min-w-0 flex-1 items-center gap-2 rounded-l-lg px-3 text-left transition-colors hover:bg-[#f7f8fb]"
            aria-label={t("Expand actor activity")}
            aria-expanded={false}
            onClick={() => setOpen(true)}
          >
            <ChevronRight size={14} className="shrink-0 text-[#667085]" />
            {compactAgent && (
              <span
                className={cn("h-2.5 w-2.5 shrink-0 rounded-full", dotClass(compactAgent.status))}
                aria-hidden
              />
            )}
            {!compactAgent && primaryService && (
              <span
                className={cn(
                  "h-2.5 w-2.5 shrink-0 rounded-full",
                  serviceDotClass(primaryService.phase),
                )}
                aria-hidden
              />
            )}
            {compactAgent ? (
              <span className="flex min-w-0 flex-1 items-baseline gap-2">
                <span className="truncate font-bold text-[#303849]">
                  {displayName(compactAgent.actor)}
                </span>
                <span className="truncate font-semibold text-[#667085]">
                  {statusText(compactAgent, t)}
                </span>
                {otherActorCount > 0 && (
                  <span className="shrink-0 font-semibold text-[#8a93a5]">
                    {t("+{{count}} others", { count: otherActorCount })}
                  </span>
                )}
                {activity.pendingTotal > 0 && compactAgent.pending === 0 && (
                  <span className="shrink-0 font-semibold text-[#8a93a5]">
                    · {t("{{count}} queued", { count: activity.pendingTotal })}
                  </span>
                )}
              </span>
            ) : primaryService ? (
              <span className="flex min-w-0 flex-1 items-baseline gap-2">
                <span className="truncate font-bold text-[#303849]">
                  {displayName(primaryService.actor)}
                </span>
                <span className="truncate font-semibold text-[#667085]">
                  {servicePhaseText(primaryService.phase, t)}
                </span>
                {otherActorCount > 0 && (
                  <span className="shrink-0 font-semibold text-[#8a93a5]">
                    {t("+{{count}} others", { count: otherActorCount })}
                  </span>
                )}
              </span>
            ) : (
              <span className="min-w-0 flex-1 truncate font-semibold">{summary}</span>
            )}
          </button>
          {compactAgent?.activeRun && (
            <StopRunButton
              key={compactAgent.activeRun.id}
              compact
              label={compactAgent.status === "unknown" ? t("Force stop") : t("Stop")}
              confirmLabel={compactAgent.status === "unknown" ? t("Force stop?") : t("Stop?")}
              title={
                compactAgent.status === "unknown"
                  ? t("Force stop {{name}}'s run", { name: displayName(compactAgent.actor) })
                  : t("Stop {{name}}'s run", { name: displayName(compactAgent.actor) })
              }
              busy={activity.busyAction === `stop:${compactAgent.activeRun.id}`}
              disabled={actionDisabled}
              className="ml-2"
              onConfirm={() => void activity.stopRun(compactAgent.activeRun!.id)}
            />
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="actor-activity-banner shrink-0 border-t border-[#e2e6ef] bg-white px-5 py-2">
      <div className="mx-auto max-w-4xl rounded-xl border border-[#e2e6ef] bg-[#fbfcff] shadow-sm">
        <button
          type="button"
          className="flex min-h-9 w-full items-center gap-2 border-b border-[#edf0f5] px-3 py-2 text-left text-xs font-bold text-[#303849]"
          aria-expanded
          onClick={() => setOpen(false)}
        >
          <ChevronDown size={14} className="shrink-0 text-[#667085]" />
          <span className="min-w-0 flex-1 truncate">{t("Actor activity")}</span>
          <span className="shrink-0 font-semibold text-[#667085]">{summary}</span>
        </button>
        {activity.error && (
          <div className="border-b border-red-100 bg-red-50 px-3 py-1.5 text-xs font-medium text-red-700">
            {t(activity.error)}
          </div>
        )}
        <div className="max-h-[40vh] divide-y divide-[#edf0f5] overflow-y-auto soft-scrollbar">
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
                      {statusText(agent, t)}
                    </span>
                    {agent.status === "running" && agent.pending > 0 && (
                      <span className="shrink-0 text-xs font-semibold text-[#8a93a5]">
                        · {t("{{count}} queued", { count: agent.pending })}
                      </span>
                    )}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  {agent.activeRun && (
                    <StopRunButton
                      compact
                      label={agent.status === "unknown" ? t("Force stop") : t("Stop")}
                      confirmLabel={agent.status === "unknown" ? t("Force stop?") : t("Stop?")}
                      disabled={actionDisabled}
                      title={agent.status === "unknown" ? t("Force stop run") : t("Stop run")}
                      busy={activity.busyAction === `stop:${agent.activeRun.id}`}
                      onConfirm={() => void activity.stopRun(agent.activeRun!.id)}
                    />
                  )}
                  {agent.pending > 0 && (
                    <>
                      <ActionButton
                        disabled={actionDisabled}
                        onClick={() => void activity.cancelAll(agent.actorId)}
                      >
                        {t("Cancel all")}
                      </ActionButton>
                      <ActionButton
                        disabled={actionDisabled || agent.queueLoading}
                        onClick={() => activity.toggleAgentQueue(agent.actorId)}
                      >
                        {agent.expanded ? t("Collapse") : t("Expand")}
                      </ActionButton>
                    </>
                  )}
                </div>
              </div>
              {agent.expanded && (
                <div className="mt-2 space-y-1 rounded-lg border border-[#edf0f5] bg-white p-2">
                  {agent.queueLoading ? (
                    <div className="px-2 py-1 text-xs font-medium text-[#667085]">
                      {t("Loading queued messages…")}
                    </div>
                  ) : agent.queueError ? (
                    <div className="px-2 py-1 text-xs font-medium text-red-600">
                      {t(agent.queueError)}
                    </div>
                  ) : agent.queue.length === 0 ? (
                    <div className="px-2 py-1 text-xs font-medium text-[#667085]">
                      {t("No queued messages.")}
                    </div>
                  ) : (
                    agent.queue.map((item) => (
                      <div
                        key={item.sourceId}
                        className="agent-queue-row grid min-h-8 grid-cols-[4.5rem_4.5rem_minmax(0,1fr)_auto] items-center gap-2 rounded-md px-2 py-1 text-xs text-[#485063] hover:bg-[#f7f8fb]"
                      >
                        <span className="font-medium text-[#8a93a5]">
                          {formatTime(item.updatedAt) || "—"}
                        </span>
                        <span className="font-semibold text-[#303849]">
                          {item.sourceKind}
                        </span>
                        <span className="truncate font-mono text-[#667085]" title={item.sourceId}>
                          {item.sourceId}
                        </span>
                        <span className="flex shrink-0 items-center gap-1.5">
                          <ActionButton
                            disabled={actionDisabled}
                            onClick={() => void activity.expedite(agent.actorId, item.sourceId)}
                          >
                            {t("Send now")}
                          </ActionButton>
                          <ActionButton
                            disabled={actionDisabled}
                            onClick={() => void activity.cancelOne(agent.actorId, item.sourceId)}
                          >
                            {t("Cancel")}
                          </ActionButton>
                        </span>
                      </div>
                    ))
                  )}
                </div>
              )}
            </div>
          ))}
          {services.map((service) => (
            <div
              key={service.key}
              data-actor-activity-kind="service"
              data-service-id={service.serviceId}
              className="px-3 py-2"
            >
              <div className="flex min-w-0 items-start gap-2">
                <span
                  className={cn(
                    "mt-1 h-2.5 w-2.5 shrink-0 rounded-full",
                    serviceDotClass(service.phase),
                  )}
                  aria-hidden
                />
                <div className="min-w-0 flex-1">
                  <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5">
                    <span className="truncate text-sm font-bold text-[#303849]">
                      {displayName(service.actor)}
                    </span>
                    <span className="rounded bg-[#eef2f8] px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide text-[#667085]">
                      {t("Service")}
                    </span>
                    <span className="shrink-0 text-xs font-semibold text-[#667085]">
                      {servicePhaseText(service.phase, t)}
                    </span>
                  </div>
                  <div className="mt-0.5 flex flex-wrap gap-x-2 text-[11px] font-medium text-[#8a93a5]">
                    <span>{t("Host: {{host}}", { host: service.machineName })}</span>
                    <span>{t("Plugin: {{plugin}}", { plugin: service.pluginKind })}</span>
                    <span>{serviceLifecycleText(service.lifecycle, t)}</span>
                    <span>{serviceInstanceText(service.instances, t)}</span>
                  </div>
                  {service.lastError && (
                    <div className="mt-1 text-xs font-medium text-red-600">
                      {t("Last error: {{error}}", { error: service.lastError })}
                    </div>
                  )}
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/** @deprecated Use ActorActivityBanner. */
export const AgentActivityBanner = ActorActivityBanner;
