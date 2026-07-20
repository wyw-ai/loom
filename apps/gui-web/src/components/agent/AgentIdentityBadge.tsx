import { type ReactNode, useState } from "react";
import {
  ChevronDown,
  ChevronUp,
  Coins,
  Database,
  Gauge,
  HeartPulse,
  Sparkles,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { AgentProviderIcon } from "@/components/agent/AgentProviderIcon";
import type { RunStatus } from "@/ipc/types";
import type { UsageDisplayState } from "@/lib/agent-identity-utils";

import "./AgentIdentityBadge.css";

export type AgentIdentityBadgeSegment = {
  id: string;
  label?: string;
  detail?: string;
  color: string;
  width: number;
  value?: string;
  percent?: string;
};

export type AgentIdentityBadgeProps = {
  avatarUrl?: string;
  agentName?: string;
  providerName?: string;
  providerMark?: string;
  providerIcon?: ReactNode;
  modelName?: string;
  usedTokensLabel?: string;
  remainingLabel?: string;
  segments?: AgentIdentityBadgeSegment[];
  /** Input X · Output Y label; hidden when undefined. */
  inputOutputLabel?: string;
  /** "Cache saved X tokens" label; hidden when undefined (PRD AC-8). */
  cacheBenefitLabel?: string;
  /** "$X.YY" label; hidden when undefined (PRD AC-7). */
  costLabel?: string;
  /** "X tokens left" label; hidden when undefined. */
  tokensLeftLabel?: string;
  /** When true, the badge is showing estimated/heuristic numbers (PRD AC-4). */
  estimated?: boolean;
  /**
   * Whether dynamic usage data is available for this agent. When false the
   * detail card shows the "Token tracking unavailable" placeholder per
   * PRD §4.2 anomaly rule.
   */
  hasUsageData?: boolean;
  /**
   * Three-state usage display (Iter#5 Part D). When provided, overrides
   * `hasUsageData` for the usage-region rendering logic:
   * - `available`: render existing segment bar (no regression)
   * - `no_data`: show "No usage data yet" (agent hasn't completed a turn)
   * - `estimated_only`: silently hide the usage region (provider doesn't
   *   return real token data)
   */
  usageState?: UsageDisplayState;
  online?: boolean;
  working?: boolean;
  workingLabel?: string;
  runStatus?: RunStatus | null;
  isStale?: boolean;
  isTerminal?: boolean;
  className?: string;
  defaultExpanded?: boolean;
  onAvatarClick?: () => void;
  onExpandedChange?: (expanded: boolean) => void;
};

// Workbench / Storybook-style preview values. Used when the badge renders
// without dynamic usage data (e.g. the standalone AgentIdentityBadgeWorkbench
// route). Production usage now derives segments from the live usage snapshot.
const defaultSegments: AgentIdentityBadgeSegment[] = [
  {
    id: "system_prompt",
    label: "System Prompt",
    detail: "Agent persona & rules",
    color: "#ff737b",
    width: 12,
    value: "13.8K",
    percent: "12%",
  },
  {
    id: "context_history",
    label: "Context History",
    detail: "Prior turns & messages",
    color: "#54afe8",
    width: 38,
    value: "44.1K",
    percent: "38%",
  },
  {
    id: "tool_definitions",
    label: "Tool Definitions",
    detail: "Function & tool schemas",
    color: "#ff9d35",
    width: 14,
    value: "16.2K",
    percent: "14%",
  },
  {
    id: "tool_results",
    label: "Tool Results",
    detail: "Tool call outputs",
    color: "#72c76a",
    width: 18,
    value: "21.0K",
    percent: "18%",
  },
  {
    id: "completions",
    label: "Completions",
    detail: "Model output tokens",
    color: "#a981e6",
    width: 18,
    value: "20.4K",
    percent: "18%",
  },
];

const runStatusLabels: Record<string, string> = {
  queued: "Queued",
  preparing_context: "Preparing",
  running: "Thinking",
  waiting_tool: "Waiting for tool",
  failed: "Failed",
  canceled: "Canceled",
};

function runStatusModifierClass(status: RunStatus | null | undefined, isStale?: boolean, isTerminal?: boolean): string {
  if (isTerminal) {
    return status === "failed"
      ? "agent-identity-badge__status--failed"
      : "agent-identity-badge__status--canceled";
  }
  if (isStale) return "agent-identity-badge__status--stale-warning";
  switch (status) {
    case "queued": return "agent-identity-badge__status--queued";
    case "preparing_context": return "agent-identity-badge__status--preparing-context";
    case "running": return "agent-identity-badge__status--running";
    case "waiting_tool": return "agent-identity-badge__status--waiting-tool";
    default: return "";
  }
}

function detailCardStatusClass(status: RunStatus | null | undefined, isStale?: boolean, isTerminal?: boolean): string {
  if (isTerminal) {
    return status === "failed"
      ? "agent-detail-card__active--failed"
      : "agent-detail-card__active--canceled";
  }
  if (isStale) return "agent-detail-card__active--stale-warning";
  switch (status) {
    case "queued": return "agent-detail-card__active--queued";
    case "preparing_context": return "agent-detail-card__active--preparing-context";
    case "running": return "agent-detail-card__active--running";
    case "waiting_tool": return "agent-detail-card__active--waiting-tool";
    default: return "";
  }
}

export function AgentIdentityBadge({
  avatarUrl = "/avatars/avatar-01.png",
  agentName = "Aiden Brooks",
  providerName = "Claude Code",
  providerMark = "AI",
  providerIcon,
  modelName = "Claude 4 Sonnet",
  usedTokensLabel = "112.0K / 128.0K tokens",
  remainingLabel = "13%",
  segments,
  inputOutputLabel,
  cacheBenefitLabel,
  costLabel,
  tokensLeftLabel,
  estimated = false,
  hasUsageData,
  usageState,
  online = true,
  working: _working = false,
  workingLabel,
  runStatus,
  isStale = false,
  isTerminal = false,
  className,
  defaultExpanded = false,
  onAvatarClick,
  onExpandedChange,
}: AgentIdentityBadgeProps) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  // When `hasUsageData` is undefined (legacy callers / standalone workbench)
  // we treat caller-provided segments as the visible truth and fall back to
  // the static workbench segments when none are supplied.
  const segmentsToRender: AgentIdentityBadgeSegment[] = segments
    ? segments
    : hasUsageData === false
      ? []
      : defaultSegments;
  const derivedWorking = runStatus != null && !isTerminal;
  const derivedWorkingLabel = workingLabel ?? (runStatus ? (runStatusLabels[runStatus] ?? "Processing…") : "Processing…");
  const activeLabel = derivedWorking ? derivedWorkingLabel : isTerminal ? (runStatusLabels[runStatus ?? ""] ?? "Terminated") : online ? "Active" : "Idle";
  function updateExpanded(nextExpanded: boolean) {
    setExpanded(nextExpanded);
    onExpandedChange?.(nextExpanded);
  }

  if (expanded) {
    return (
      <AgentIdentityDetailCard
        avatarUrl={avatarUrl}
        agentName={agentName}
        providerName={providerName}
        providerMark={providerMark}
        providerIcon={providerIcon}
        modelName={modelName}
        usedTokensLabel={usedTokensLabel}
        remainingLabel={remainingLabel}
        segments={segmentsToRender}
        inputOutputLabel={inputOutputLabel}
        cacheBenefitLabel={cacheBenefitLabel}
        costLabel={costLabel}
        tokensLeftLabel={tokensLeftLabel}
        estimated={estimated}
        hasUsageData={hasUsageData ?? segmentsToRender.length > 0}
        usageState={usageState}
        activeLabel={activeLabel}
        working={derivedWorking}
        runStatus={runStatus}
        isStale={isStale}
        isTerminal={isTerminal}
        className={className}
        onAvatarClick={onAvatarClick}
        onCollapse={() => updateExpanded(false)}
      />
    );
  }

  const statusMod = runStatusModifierClass(runStatus, isStale, isTerminal);

  return (
    <article className={cn("agent-identity-badge", className)} aria-label={`${agentName} agent badge`}>
      {derivedWorking && (
        <span
          className={cn("agent-identity-badge__status agent-identity-badge__status--working", statusMod)}
          aria-label={derivedWorkingLabel}
        />
      )}
      {!derivedWorking && isTerminal && (
        <span
          className={cn("agent-identity-badge__status", statusMod)}
          aria-label={runStatusLabels[runStatus ?? ""] ?? "Terminated"}
        />
      )}
      {!derivedWorking && !isTerminal && online && <span className="agent-identity-badge__status" aria-label="Active" />}
      {onAvatarClick ? (
        <button
          type="button"
          className="agent-identity-badge__avatar-button"
          aria-label={`Open ${agentName} settings`}
          onClick={onAvatarClick}
        >
          <img className="agent-identity-badge__avatar" src={avatarUrl} alt="" />
        </button>
      ) : (
        <img className="agent-identity-badge__avatar" src={avatarUrl} alt="" />
      )}
      <h2 className="agent-identity-badge__name">{agentName}</h2>

      <div className="agent-identity-badge__provider">
        <span className="agent-identity-badge__provider-mark" aria-hidden="true">
          {providerIcon ?? providerMark}
        </span>
        <span className="agent-identity-badge__provider-name">{providerName}</span>
      </div>

      <h2 className="agent-identity-badge__model">{modelName}</h2>

      <div
        className={cn(
          "agent-identity-badge__meter",
          estimated && "agent-identity-badge__meter--estimated",
        )}
        aria-hidden="true"
      >
        {segmentsToRender.map((segment) => (
          <span
            key={segment.id}
            className="agent-identity-badge__segment"
            style={{ backgroundColor: segment.color, flex: `0 0 ${segment.width}%` }}
          />
        ))}
      </div>

      <div
        className={cn(
          "agent-identity-badge__usage",
          estimated && "agent-identity-badge__usage--estimated",
        )}
      >
        <span>{usedTokensLabel}</span>
        {remainingLabel ? (
          <span className="agent-identity-badge__remaining">{remainingLabel}</span>
        ) : null}
      </div>

      <button
        type="button"
        className="agent-identity-badge__chevron"
        aria-label="Open agent details"
        aria-expanded={false}
        onClick={() => updateExpanded(true)}
      >
        <ChevronDown size={30} strokeWidth={2.25} />
      </button>
    </article>
  );
}

function AgentIdentityDetailCard({
  avatarUrl,
  agentName,
  providerName,
  providerMark,
  providerIcon,
  modelName,
  usedTokensLabel,
  remainingLabel,
  segments,
  inputOutputLabel,
  cacheBenefitLabel,
  costLabel,
  tokensLeftLabel,
  estimated = false,
  hasUsageData = true,
  usageState,
  activeLabel,
  working = false,
  runStatus,
  isStale = false,
  isTerminal = false,
  className,
  onAvatarClick,
  onCollapse,
}: {
  avatarUrl: string;
  agentName: string;
  providerName: string;
  providerMark: string;
  providerIcon?: ReactNode;
  modelName: string;
  usedTokensLabel: string;
  remainingLabel: string;
  segments: AgentIdentityBadgeSegment[];
  inputOutputLabel?: string;
  cacheBenefitLabel?: string;
  costLabel?: string;
  tokensLeftLabel?: string;
  estimated?: boolean;
  hasUsageData?: boolean;
  usageState?: UsageDisplayState;
  activeLabel: string;
  working?: boolean;
  runStatus?: RunStatus | null;
  isStale?: boolean;
  isTerminal?: boolean;
  className?: string;
  onAvatarClick?: () => void;
  onCollapse: () => void;
}) {
  const statusClass = runStatus ? detailCardStatusClass(runStatus, isStale, isTerminal) : "";
  const hasTokenStats =
    Boolean(inputOutputLabel) || Boolean(cacheBenefitLabel) || Boolean(costLabel);
  // Iter#5 Part D — three-state usage rendering.
  // estimated_only: silently hide the entire usage region (both sections).
  const hideUsageRegion = usageState?.kind === "estimated_only";
  // no_data: show "No usage data yet" instead of "Token tracking unavailable..."
  const usageEmptyText =
    usageState?.kind === "no_data"
      ? "No usage data yet"
      : hasUsageData
        ? "Token statistics will appear once this agent finishes a turn."
        : "Token tracking unavailable for this provider.";
  return (
    <article className={cn("agent-detail-card", className)} aria-label={`${agentName} agent details`}>
      <div className="agent-detail-card__hero">
        {onAvatarClick ? (
          <button
            type="button"
            className="agent-detail-card__avatar-button"
            aria-label={`Open ${agentName} settings`}
            onClick={onAvatarClick}
          >
            <img className="agent-detail-card__avatar" src={avatarUrl} alt="" />
          </button>
        ) : (
          <img className="agent-detail-card__avatar" src={avatarUrl} alt="" />
        )}

        <div className="agent-detail-card__summary">
          <span className="agent-detail-card__type">
            <Sparkles size={18} strokeWidth={2.15} />
            AI Agent
          </span>

          <div className="agent-detail-card__identity">
            <div className="agent-detail-card__label">AGENT NAME</div>
            <h1>{agentName}</h1>
          </div>

          <div className="agent-detail-card__field">
            <div className="agent-detail-card__label">AGENT RUNTIME</div>
            <div className="agent-detail-card__provider">
              <span className="agent-detail-card__provider-mark" aria-hidden="true">
                {providerIcon ?? providerMark}
              </span>
              <span>{providerName}</span>
            </div>
          </div>

          <div className="agent-detail-card__field agent-detail-card__field-model">
            <div className="agent-detail-card__label">MODEL</div>
            <h2>{modelName}</h2>
          </div>

          <span className={cn("agent-detail-card__active", working && "agent-detail-card__active--working", statusClass)}>
            <span />
            {activeLabel}
          </span>
        </div>
      </div>

      {hideUsageRegion ? null : (
      <>
      <section className="agent-detail-section agent-detail-traits">
        <div className="agent-detail-section__heading">
          <span className="agent-detail-section__icon">
            <Gauge size={17} strokeWidth={2.05} />
          </span>
          <span>TOKEN STATS</span>
          {estimated ? (
            <span
              className="agent-detail-traits__estimated"
              title="Some values are estimated (~)"
            >
              estimated
            </span>
          ) : null}
        </div>
        <div className="agent-detail-traits__content">
          <div className="agent-detail-traits__copy">
            {hasTokenStats ? (
              <ul className="agent-detail-traits__stats">
                {inputOutputLabel ? (
                  <li>
                    <span className="agent-detail-traits__stats-label">Throughput</span>
                    <strong>{inputOutputLabel}</strong>
                  </li>
                ) : null}
                {cacheBenefitLabel ? (
                  <li>
                    <span className="agent-detail-traits__stats-label">Cache</span>
                    <strong>{cacheBenefitLabel}</strong>
                  </li>
                ) : null}
                {costLabel ? (
                  <li>
                    <span className="agent-detail-traits__stats-label">Cost</span>
                    <strong>{costLabel}</strong>
                  </li>
                ) : null}
              </ul>
            ) : (
              <p className="agent-detail-traits__empty">
                {usageEmptyText}
              </p>
            )}
          </div>

          <div className="agent-detail-traits__brain" aria-hidden="true">
            <Coins size={72} strokeWidth={1.55} />
          </div>
        </div>
      </section>

      <section className="agent-detail-section agent-detail-context">
        <div className="agent-detail-section__title-row">
          <div className="agent-detail-section__heading">
            <span className="agent-detail-section__icon">
              <Database size={17} strokeWidth={2.15} />
            </span>
            <span>CONTEXT BREAKDOWN</span>
          </div>
          <button
            type="button"
            className="agent-detail-section__collapse"
            aria-label="Collapse agent details"
            aria-expanded={true}
            onClick={onCollapse}
          >
            <ChevronUp size={23} strokeWidth={2.25} />
          </button>
        </div>

        {segments.length > 0 ? (
          <>
            <div
              className={cn(
                "agent-detail-context__meter",
                estimated && "agent-detail-context__meter--estimated",
              )}
              aria-hidden="true"
            >
              {segments.map((segment) => (
                <span
                  key={segment.id}
                  style={{ backgroundColor: segment.color, flex: `0 0 ${segment.width}%` }}
                />
              ))}
            </div>

            <div className="agent-detail-context__table">
              {segments.map((segment) => (
                <div key={segment.id} className="agent-detail-context__row">
                  <span className="agent-detail-context__swatch" style={{ backgroundColor: segment.color }} />
                  <span className="agent-detail-context__name">
                    <strong>{segment.label}</strong>
                    <span>{segment.detail}</span>
                  </span>
                  <span className="agent-detail-context__value">{segment.value}</span>
                  <span className="agent-detail-context__percent">{segment.percent}</span>
                </div>
              ))}
            </div>
          </>
        ) : (
          <p className="agent-detail-context__empty">
            {usageState?.kind === "no_data"
              ? "No usage data yet"
              : hasUsageData
                ? "Breakdown will populate after the first turn completes."
                : "Token tracking unavailable for this provider."}
          </p>
        )}

        <div className="agent-detail-context__total">
          <span>Total Used</span>
          <strong>{usedTokensLabel}</strong>
        </div>
      </section>
      </>
      )}

      <section className="agent-detail-section agent-detail-hp">
        <div className="agent-detail-hp__top">
          <div className="agent-detail-section__heading">
            <span className="agent-detail-section__icon agent-detail-section__icon-hp">
              <HeartPulse size={17} strokeWidth={2.15} />
            </span>
            <span>CONTEXT HP</span>
          </div>
          {remainingLabel ? (
            <strong>{remainingLabel} <span>remaining</span></strong>
          ) : null}
        </div>

        <div className="agent-detail-hp__track" aria-hidden="true">
          <span />
        </div>

        <div className="agent-detail-hp__bottom">
          <span>{usedTokensLabel} used</span>
          {tokensLeftLabel ? <strong>{tokensLeftLabel}</strong> : null}
        </div>
      </section>
    </article>
  );
}

export function AgentIdentityBadgeWorkbench() {
  const defaultExpanded = new URLSearchParams(window.location.search).get("expanded") === "1";

  return (
    <main className="agent-identity-badge-workbench">
      <div className="agent-identity-badge-workbench__stage">
        <AgentIdentityBadge
          defaultExpanded={defaultExpanded}
          providerIcon={<AgentProviderIcon iconKey="claude" />}
        />
      </div>
    </main>
  );
}
