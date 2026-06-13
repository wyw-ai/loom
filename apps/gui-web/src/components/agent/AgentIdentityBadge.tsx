import { type ReactNode, useState } from "react";
import {
  Brain,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Database,
  HeartPulse,
  Sparkles,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { AgentProviderIcon } from "@/components/agent/AgentProviderIcon";

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
  online?: boolean;
  working?: boolean;
  workingLabel?: string;
  className?: string;
  defaultExpanded?: boolean;
  onAvatarClick?: () => void;
  onExpandedChange?: (expanded: boolean) => void;
};

const defaultSegments: AgentIdentityBadgeSegment[] = [
  {
    id: "memory",
    label: "Memory",
    detail: "Agent's long-term memory",
    color: "#a981e6",
    width: 22,
    value: "24.3K",
    percent: "22%",
  },
  {
    id: "files",
    label: "Files",
    detail: "Uploaded documents & data",
    color: "#54afe8",
    width: 29,
    value: "31.6K",
    percent: "29%",
  },
  {
    id: "chat",
    label: "Chat History",
    detail: "Recent conversations",
    color: "#72c76a",
    width: 24,
    value: "27.2K",
    percent: "24%",
  },
  {
    id: "tools",
    label: "Tools",
    detail: "Functions & tool definitions",
    color: "#ff9d35",
    width: 13,
    value: "15.1K",
    percent: "13%",
  },
  {
    id: "system",
    label: "System Context",
    detail: "Instructions & system prompts",
    color: "#ff737b",
    width: 12,
    value: "13.8K",
    percent: "12%",
  },
];

export function AgentIdentityBadge({
  avatarUrl = "/avatars/avatar-01.png",
  agentName = "Aiden Brooks",
  providerName = "Claude Code",
  providerMark = "AI",
  providerIcon,
  modelName = "Claude 4 Sonnet",
  usedTokensLabel = "112.0K / 128.0K tokens",
  remainingLabel = "13%",
  segments = defaultSegments,
  online = true,
  working = false,
  workingLabel = "Processing…",
  className,
  defaultExpanded = false,
  onAvatarClick,
  onExpandedChange,
}: AgentIdentityBadgeProps) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  const activeLabel = working ? workingLabel : online ? "Active" : "Idle";
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
        segments={segments}
        activeLabel={activeLabel}
        working={working}
        className={className}
        onAvatarClick={onAvatarClick}
        onCollapse={() => updateExpanded(false)}
      />
    );
  }

  return (
    <article className={cn("agent-identity-badge", className)} aria-label={`${agentName} agent badge`}>
      {working && <span className="agent-identity-badge__status agent-identity-badge__status--working" aria-label={workingLabel} />}
      {!working && online && <span className="agent-identity-badge__status" aria-label="Active" />}
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

      <div className="agent-identity-badge__meter" aria-hidden="true">
        {segments.map((segment) => (
          <span
            key={segment.id}
            className="agent-identity-badge__segment"
            style={{ backgroundColor: segment.color, flex: `0 0 ${segment.width}%` }}
          />
        ))}
      </div>

      <div className="agent-identity-badge__usage">
        <span>{usedTokensLabel}</span>
        <span className="agent-identity-badge__remaining">{remainingLabel}</span>
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
  activeLabel,
  working = false,
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
  activeLabel: string;
  working?: boolean;
  className?: string;
  onAvatarClick?: () => void;
  onCollapse: () => void;
}) {
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

          <span className={cn("agent-detail-card__active", working && "agent-detail-card__active--working")}>
            <span />
            {activeLabel}
          </span>
        </div>
      </div>

      <section className="agent-detail-section agent-detail-traits">
        <div className="agent-detail-section__heading">
          <span className="agent-detail-section__icon">
            <Sparkles size={17} strokeWidth={2.05} />
          </span>
          <span>PERSONAL TRAITS</span>
        </div>
        <div className="agent-detail-traits__content">
          <div className="agent-detail-traits__copy">
            <div className="agent-detail-traits__chips">
              <span>Thoughtful</span>
              <span className="agent-detail-traits__dot agent-detail-traits__dot-orange" />
              <span>Analytical</span>
              <span className="agent-detail-traits__dot agent-detail-traits__dot-green" />
              <span>Reliable</span>
            </div>
            <p>
              Calm, precise, and context-aware. Excels at reasoning, summarization, and
              complex problem solving.
            </p>
            <button type="button" className="agent-detail-traits__link">
              Read more
              <ChevronRight size={18} strokeWidth={2.4} />
            </button>
          </div>

          <div className="agent-detail-traits__brain" aria-hidden="true">
            <span className="agent-detail-traits__spark agent-detail-traits__spark-one" />
            <span className="agent-detail-traits__spark agent-detail-traits__spark-two" />
            <span className="agent-detail-traits__spark agent-detail-traits__spark-three" />
            <Brain size={82} strokeWidth={1.55} />
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

        <div className="agent-detail-context__meter" aria-hidden="true">
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

        <div className="agent-detail-context__total">
          <span>Total Used</span>
          <strong>{usedTokensLabel}</strong>
        </div>
      </section>

      <section className="agent-detail-section agent-detail-hp">
        <div className="agent-detail-hp__top">
          <div className="agent-detail-section__heading">
            <span className="agent-detail-section__icon agent-detail-section__icon-hp">
              <HeartPulse size={17} strokeWidth={2.15} />
            </span>
            <span>CONTEXT HP</span>
          </div>
          <strong>{remainingLabel} <span>remaining</span></strong>
        </div>

        <div className="agent-detail-hp__track" aria-hidden="true">
          <span />
        </div>

        <div className="agent-detail-hp__bottom">
          <span>{usedTokensLabel} used</span>
          <strong>16.0K tokens left</strong>
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
