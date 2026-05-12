import { useEffect, useState } from "react";
import clsx from "clsx";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Clock3,
  Copy,
  CornerDownRight,
  Fingerprint,
  Bookmark,
  BarChart3,
  XCircle,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import * as ipc from "@/ipc/bridge";

function formatChatTime(ts: string | number): string {
  const d = new Date(ts);
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  if (d.toDateString() === now.toDateString()) {
    return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
import { scopeKey, type Bubble as BubbleT } from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useChannels } from "@/store/channels";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { ActorAvatar } from "@/features/common/ActorAvatar";
import { ArtifactAttachments } from "./ArtifactAttachment";

export interface BubbleReplyContext {
  actorId: string;
  preview: string;
  domId?: string;
  missing?: boolean;
}

export function Bubble({
  bubble,
  showHeader,
  domId,
  replyContext,
}: {
  bubble: BubbleT;
  showHeader: boolean;
  domId: string;
  replyContext?: BubbleReplyContext;
}) {
  const actorId = bubble.actorId;
  const selfId = useSession((s) => s.workspace?.actorId);
  const actor = useActors((s) => s.byId[actorId]);
  const replyAuthor = useActors((s) =>
    replyContext ? s.byId[replyContext.actorId] : undefined,
  );
  const currentScope = useChannels((s) => s.currentScope);
  const setReplyTarget = useUI((s) => s.setReplyTarget);
  const openContextMenu = useUI((s) => s.openContextMenu);
  const pushToast = useUI((s) => s.pushToast);
  const isSelf = actorId === selfId;

  if (bubble.kind === "system") {
    return (
      <div
        id={domId}
        className="my-1 break-words scroll-mt-16 text-center font-mono text-xs italic text-black/40"
      >
        {bubble.text}
      </div>
    );
  }

  const displayName = actor?.displayName || actorId;

  const replyToThis = () => {
    if (!currentScope) return;
    setReplyTarget(currentScope, {
      eventId: bubble.id,
      actorId: bubble.actorId,
      preview: previewText(bubble.text, bubble.handoffTarget),
      handoffTarget: bubble.handoffTarget,
    });
    window.dispatchEvent(
      new CustomEvent("joi:focus-prompt", {
        detail: { scopeKey: scopeKey(currentScope) },
      }),
    );
  };

  const copyText = async () => {
    try {
      await navigator.clipboard.writeText(bubble.text);
      pushToast("info", "copied to clipboard");
    } catch (e) {
      pushToast(
        "warn",
        `copy failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  };

  const revealId = async () => {
    try {
      await navigator.clipboard.writeText(actorId);
      pushToast("info", `${actorId} · copied`);
    } catch {
      pushToast("info", actorId);
    }
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    openContextMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { kind: "item", label: "Reply", onClick: replyToThis },
        { kind: "item", label: "Copy text", onClick: () => void copyText() },
        {
          kind: "item",
          label: `Copy actor id (${actorId})`,
          onClick: () => void revealId(),
        },
      ],
    });
  };

  const jumpToReply = () => {
    if (!replyContext?.domId) return;
    const target = document.getElementById(replyContext.domId);
    if (!target) return;
    target.scrollIntoView({ block: "center", behavior: "smooth" });
    target.classList.add("bubble-flash");
    window.setTimeout(() => target.classList.remove("bubble-flash"), 900);
  };

  return (
    <div
      id={domId}
      className="group relative flex min-w-0 scroll-mt-16 gap-3 px-2 py-1 transition-colors hover:bg-brutal-cream focus-within:bg-brutal-cream"
      onContextMenu={onContextMenu}
    >
      <div className="w-10 shrink-0">
        {showHeader && (
          <button
            type="button"
            onClick={() => void revealId()}
            title={actorId}
            className="block outline-none focus-visible:ring-2 focus-visible:ring-black"
          >
            <ActorAvatar actor={actor} id={actorId} label={displayName} size={36} />
          </button>
        )}
      </div>
      <div className="min-w-0 flex-1">
        {showHeader && (
          <header className="flex items-baseline gap-2">
            <button
              type="button"
              onClick={() => void revealId()}
              title={actorId}
              className={clsx(
                "text-sm font-black outline-none hover:underline focus-visible:underline",
                isSelf ? "text-primary" : roleColor(actorId),
              )}
            >
              {displayName}
            </button>
            <span className="font-mono text-[11px] text-black/40">
              {formatChatTime(bubble.ts)}
            </span>
            {bubble.delivery === "pending" && (
              <span className="font-mono text-[11px] text-black/40">sending...</span>
            )}
          </header>
        )}

        {replyContext && (
          <button
            type="button"
            disabled={replyContext.missing}
            className="mb-0.5 flex max-w-full items-center gap-1.5 text-left text-xs text-black/45 hover:text-black/70 disabled:cursor-default disabled:hover:text-black/45"
            onClick={jumpToReply}
            title={replyContext.preview}
          >
            <CornerDownRight size={12} className="shrink-0" />
            <span
              className={clsx(
                "max-w-[9rem] truncate font-semibold",
                replyContext.missing
                  ? "text-muted"
                  : roleColor(replyContext.actorId),
              )}
            >
              {replyContext.missing
                ? "Unknown"
                : replyAuthor?.displayName || replyContext.actorId}
            </span>
            <span className="min-w-0 flex-1 truncate">
              {replyContext.preview}
            </span>
          </button>
        )}

        <Body bubble={bubble} />
        <ArtifactAttachments artifactIds={bubble.attachmentIds} />
        <MessageMeta meta={bubble.meta} />
      </div>

      {/* Hover toolbar — floats just above the top-right, Discord-style.
          It mirrors the context menu for the fast reply/copy path. */}
      <div className="pointer-events-none absolute -top-3 right-3 flex items-center gap-0.5 border-2 border-black bg-white px-0.5 py-0.5 opacity-0 shadow-brutal-sm transition-opacity duration-75 group-hover:pointer-events-auto group-hover:opacity-100">
        <button
          type="button"
          title="Save message"
          onClick={() => pushToast("info", "message saved")}
          className="flex h-6 w-6 items-center justify-center text-black/70 hover:bg-brutal-yellow hover:text-black"
        >
          <Bookmark size={13} />
        </button>
        <button
          type="button"
          title="Reply"
          onClick={replyToThis}
          className="flex h-6 w-6 items-center justify-center text-black/70 hover:bg-brutal-yellow hover:text-black"
        >
          <CornerDownRight size={14} />
        </button>
        <button
          type="button"
          title="Copy text"
          onClick={() => void copyText()}
          className="flex h-6 w-6 items-center justify-center text-black/70 hover:bg-brutal-yellow hover:text-black"
        >
          <Copy size={14} />
        </button>
        <button
          type="button"
          title="Copy actor id"
          onClick={() => void revealId()}
          className="flex h-6 w-6 items-center justify-center text-black/70 hover:bg-brutal-yellow hover:text-black"
        >
          <Fingerprint size={14} />
        </button>
      </div>
    </div>
  );
}

function previewText(text: string, handoffTarget?: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  const head = handoffTarget ? `→@${handoffTarget}: ` : "";
  const body = flat.length > 64 ? flat.slice(0, 63) + "…" : flat;
  return `${head}${body}` || "(no text)";
}

function roleColor(actorId: string): string {
  if (actorId.startsWith("actor_agent_")) return "text-black";
  if (actorId.startsWith("actor_service_")) return "text-black/70";
  if (actorId === "system") return "text-black/45";
  return "text-black";
}

function Body({ bubble }: { bubble: BubbleT }) {
  switch (bubble.kind) {
    case "actionRequest":
      return <ActionRequestBody bubble={bubble} />;
    case "static":
      if (bubble.handoffTarget) {
        return <HandoffBody bubble={bubble} />;
      }
      return <MarkdownText text={bubble.text} />;
    case "stream":
    default:
      return (
        <div className="relative">
          <MarkdownText text={bubble.text} />
          {bubble.streaming && (
            <span className="ml-1 inline-block h-[1em] w-[2px] animate-pulse bg-accent align-middle" />
          )}
        </div>
      );
  }
}

function HandoffBody({ bubble }: { bubble: BubbleT }) {
  const target = bubble.handoffTarget!;
  const targetActor = useActors((s) => s.byId[target]);
  const targetName = targetActor?.displayName || target;
  return (
    <div className="break-words text-sm text-black/70">
      <span className="text-black/45">handoff {"->"}</span>{" "}
      <span className="bg-mention px-1 font-black text-black" title={target}>
        @{targetName}
      </span>
      {bubble.text ? <>: {bubble.text}</> : null}
    </div>
  );
}

function MarkdownText({ text }: { text: string }) {
  return (
    <div className="prose-chat min-w-0 max-w-full text-sm leading-6 text-black">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  );
}

interface PromptStatsMeta {
  char_count: number;
  byte_count: number;
  approx_token_count: number;
}

interface PromptBreakdownSection {
  key: string;
  label: string;
  char_count: number;
  byte_count: number;
  approx_token_count: number;
  percentage: number;
}

interface TokenUsage {
  input_tokens?: number;
  output_tokens?: number;
  total_tokens?: number;
  total_cost_usd?: number;
  estimated?: boolean;
}

interface TokenUsageMeta {
  increment?: TokenUsage;
  cumulative?: TokenUsage;
}

function MessageMeta({ meta }: { meta?: Record<string, unknown> }) {
  const stats = getPromptStats(meta);
  const breakdown = getPromptBreakdown(meta);
  const tokenUsage = getTokenUsage(meta);
  if (!stats && !tokenUsage) return null;

  return (
    <div className="mt-1 max-w-2xl text-[11px] text-black/45">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 font-mono">
        <BarChart3 size={12} className="text-black/35" />
        {tokenUsage?.increment && (
          <span>
            {`usage ${formatTokenUsage(tokenUsage.increment, true)}`}
            {tokenUsage.cumulative
              ? ` · total ${formatTokenUsage(tokenUsage.cumulative, false)}`
              : ""}
          </span>
        )}
        {stats && (
          <span>{`prompt ctx ~${formatCompactCount(stats.approx_token_count)} tok · ${formatBytes(stats.byte_count)}`}</span>
        )}
      </div>
      {breakdown.length > 0 && (
        <details className="mt-1 max-w-xl">
          <summary className="cursor-pointer select-none font-mono text-[11px] text-black/45 hover:text-black/70">
            Context breakdown
          </summary>
          <div className="mt-1 border-l-2 border-black/20 pl-2">
            <div className="flex h-2 overflow-hidden border border-black/20 bg-black/5">
              {breakdown.map((section) => (
                <div
                  key={section.key}
                  className="h-full"
                  title={`${section.label}: ~${formatCompactCount(section.approx_token_count)} tok`}
                  style={{
                    width: `${Math.max(section.percentage, 2)}%`,
                    background: contextSectionColor(section.key),
                  }}
                />
              ))}
            </div>
            <div className="mt-1 grid gap-1">
              {breakdown.map((section) => (
                <div
                  key={section.key}
                  className="grid grid-cols-[minmax(0,1fr)_auto] gap-2 font-mono text-[11px]"
                >
                  <span className="min-w-0 truncate text-black/55">
                    <span
                      className="mr-1 inline-block h-2 w-2 border border-black/20 align-[-1px]"
                      style={{ background: contextSectionColor(section.key) }}
                    />
                    {section.label}
                  </span>
                  <span className="text-black/45">
                    {`~${formatCompactCount(section.approx_token_count)} tok · ${formatPercentage(section.percentage)}`}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </details>
      )}
    </div>
  );
}

function getPromptStats(
  meta?: Record<string, unknown>,
): PromptStatsMeta | null {
  const stats = asRecord(meta?.prompt_stats) ?? asRecord(meta?.promptStats);
  if (!stats) return null;
  const char_count = asNumber(stats.char_count ?? stats.charCount);
  const byte_count = asNumber(stats.byte_count ?? stats.byteCount);
  const approx_token_count = asNumber(
    stats.approx_token_count ?? stats.approxTokenCount,
  );
  if (
    char_count === null ||
    byte_count === null ||
    approx_token_count === null
  ) {
    return null;
  }
  return { char_count, byte_count, approx_token_count };
}

function getPromptBreakdown(
  meta?: Record<string, unknown>,
): PromptBreakdownSection[] {
  const breakdown =
    asRecord(meta?.prompt_breakdown) ?? asRecord(meta?.promptBreakdown);
  const rawSections = breakdown?.sections;
  if (!Array.isArray(rawSections)) return [];
  return rawSections
    .map((raw) => {
      const section = asRecord(raw);
      if (!section) return null;
      const key = asStringValue(section.key);
      const label = asStringValue(section.label);
      const char_count = asNumber(section.char_count ?? section.charCount);
      const byte_count = asNumber(section.byte_count ?? section.byteCount);
      const approx_token_count = asNumber(
        section.approx_token_count ?? section.approxTokenCount,
      );
      const percentage = asNumber(section.percentage);
      if (
        !key ||
        !label ||
        char_count === null ||
        byte_count === null ||
        approx_token_count === null ||
        percentage === null
      ) {
        return null;
      }
      return {
        key,
        label,
        char_count,
        byte_count,
        approx_token_count,
        percentage,
      };
    })
    .filter((section): section is PromptBreakdownSection => Boolean(section));
}

function getTokenUsage(meta?: Record<string, unknown>): TokenUsageMeta | null {
  const usage = asRecord(meta?.token_usage) ?? asRecord(meta?.tokenUsage);
  if (!usage) return null;
  const increment = parseUsage(usage.increment);
  const cumulative = parseUsage(usage.cumulative);
  if (!increment && !cumulative) return null;
  return { increment: increment ?? undefined, cumulative: cumulative ?? undefined };
}

function parseUsage(raw: unknown): TokenUsage | null {
  const usage = asRecord(raw);
  if (!usage) return null;
  const parsed: TokenUsage = {
    input_tokens: asOptionalNumber(usage.input_tokens ?? usage.inputTokens),
    output_tokens: asOptionalNumber(usage.output_tokens ?? usage.outputTokens),
    total_tokens: asOptionalNumber(usage.total_tokens ?? usage.totalTokens),
    total_cost_usd: asOptionalNumber(usage.total_cost_usd ?? usage.totalCostUsd),
    estimated: usage.estimated === true,
  };
  if (
    parsed.input_tokens === undefined &&
    parsed.output_tokens === undefined &&
    parsed.total_tokens === undefined &&
    parsed.total_cost_usd === undefined
  ) {
    return null;
  }
  return parsed;
}

function formatTokenUsage(usage: TokenUsage, includeDelta: boolean): string {
  const total =
    usage.total_tokens ??
    sumDefined([usage.input_tokens, usage.output_tokens]);
  const prefix = includeDelta ? "+" : "";
  const approx = usage.estimated ? "~" : "";
  const cost =
    typeof usage.total_cost_usd === "number"
      ? ` · $${usage.total_cost_usd.toFixed(4)}`
      : "";
  if (typeof total !== "number") return cost.trim().replace(/^· /, "") || "n/a";
  return `${prefix}${approx}${formatCompactCount(total)} tok${cost}`;
}

function sumDefined(values: Array<number | undefined>): number | undefined {
  let total = 0;
  let seen = false;
  for (const value of values) {
    if (typeof value !== "number") continue;
    total += value;
    seen = true;
  }
  return seen ? total : undefined;
}

function contextSectionColor(key: string): string {
  switch (key) {
    case "identity":
      return "var(--brutal-cyan)";
    case "soul":
      return "var(--brutal-lavender)";
    case "bootstrap_memory":
    case "turn_memory":
      return "var(--brutal-lime)";
    case "scope_bootstrap":
      return "var(--brutal-orange)";
    case "user_message":
      return "var(--brutal-pink)";
    default:
      return "var(--brutal-yellow)";
  }
}

function formatCompactCount(value: number): string {
  if (value >= 1_000_000) return `${trimFixed(value / 1_000_000)}m`;
  if (value >= 1_000) return `${trimFixed(value / 1_000)}k`;
  return String(Math.round(value));
}

function trimFixed(value: number): string {
  return value.toFixed(value >= 10 ? 0 : 1).replace(/\.0$/, "");
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${trimFixed(bytes / 1024 / 1024)} MB`;
  if (bytes >= 1024) return `${trimFixed(bytes / 1024)} KB`;
  return `${Math.round(bytes)} B`;
}

function formatPercentage(value: number): string {
  if (value >= 10) return `${Math.round(value)}%`;
  return `${value.toFixed(1).replace(/\.0$/, "")}%`;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function asNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function asOptionalNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function asStringValue(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function ActionRequestBody({ bubble }: { bubble: BubbleT }) {
  const currentScope = useChannels((s) => s.currentScope);
  const selfId = useSession((s) => s.workspace?.actorId);
  const pushToast = useUI((s) => s.pushToast);
  const disabled = bubble.acknowledged === true;

  const respond = async (
    optionId: string,
    kind: "accepted" | "declined" | "answered",
  ) => {
    if (!currentScope || !selfId) return;
    try {
      await ipc.eventAppend({
        type: "action.response",
        actorId: selfId,
        scope: currentScope,
        payload: {
          optionId,
          kind,
          ...(bubble.actionRequestId ? { requestId: bubble.actionRequestId } : {}),
        },
        relations: [
          { kind: "responds_to", target: { kind: "event", id: bubble.id } },
        ],
      });
    } catch (e) {
      pushToast(
        "error",
        `action.response failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  };

  const choices = bubble.choices ?? [
    { id: "approve", label: "Approve" },
    { id: "reject", label: "Reject" },
  ];
  const status = bubble.actionStatus ?? (disabled ? "accepted" : "pending");
  const statusTone = actionStatusTone(status);
  const StatusIcon = statusTone.icon;
  const statusLabel =
    status === "answered" && bubble.actionSelectedLabel
      ? `Selected: ${bubble.actionSelectedLabel}`
      : statusTone.label;
  const [expanded, setExpanded] = useState(() => !disabled);
  const cardShell =
    "mt-1 w-full min-w-0 max-w-3xl overflow-hidden border-2 border-black bg-white shadow-brutal-sm";
  const bodyShell = "px-3 pb-3 pt-2";
  const hasStructuredBody =
    bubble.actionReason || bubble.actionCommand || bubble.actionRawInput;
  const actionTitle =
    bubble.actionTitle ??
    (hasStructuredBody ? "Permission required" : "Action requested");

  useEffect(() => {
    if (disabled) setExpanded(false);
  }, [disabled]);

  const renderHeader = (collapsed: boolean) => (
    <div className="grid h-10 w-full grid-cols-[1.25rem_1.25rem_minmax(0,1fr)_auto] items-center gap-2 px-3">
      {disabled ? (
        collapsed ? (
          <ChevronRight size={15} className="justify-self-center text-muted" />
        ) : (
          <button
            type="button"
            onClick={() => setExpanded(false)}
            className="flex h-5 w-5 items-center justify-center text-black/45 hover:bg-brutal-yellow hover:text-black"
          >
            <ChevronDown size={14} />
          </button>
        )
      ) : (
        <span className="h-5 w-5" aria-hidden="true" />
      )}
      <StatusIcon
        size={15}
        className={clsx("justify-self-center", statusTone.iconClass)}
      />
      <div className="min-w-0 truncate text-sm font-black text-black">
        {actionTitle}
      </div>
      <span
        className={clsx(
          "max-w-[14rem] shrink-0 truncate border border-black px-1.5 py-0.5 text-[11px] font-black",
          statusTone.badge,
        )}
      >
        {statusLabel}
      </span>
    </div>
  );

  if (disabled && !expanded) {
    return (
      <button
        type="button"
        onClick={() => setExpanded(true)}
        className={clsx(
          cardShell,
          "block text-left",
          statusTone.card,
        )}
      >
        {renderHeader(true)}
      </button>
    );
  }

  return (
    <div
      className={clsx(
        cardShell,
        statusTone.card,
      )}
    >
      {renderHeader(false)}
      {hasStructuredBody ? (
        <div className={clsx(bodyShell, "space-y-2")}>
          {bubble.actionReason && (
            <div className="text-sm text-black/70">
              <div className="mb-0.5 text-[11px] font-black uppercase tracking-wider text-black/45">
                Reason
              </div>
              <div className="whitespace-pre-wrap break-words">
                {bubble.actionReason}
              </div>
            </div>
          )}
          {bubble.actionCommand && (
            <div>
              <div className="mb-1 text-[11px] font-black uppercase tracking-wider text-black/45">
                Command
              </div>
              <code className="block overflow-x-auto border-2 border-black bg-brutal-cream px-2 py-1.5 font-mono text-xs leading-5 text-black/70">
                {bubble.actionCommand}
              </code>
            </div>
          )}
          {bubble.actionRawInput && (
            <div>
              <div className="mb-1 text-[11px] font-black uppercase tracking-wider text-black/45">
                Raw input
              </div>
              <code className="block overflow-x-auto whitespace-pre border-2 border-black bg-brutal-cream px-2 py-1.5 font-mono text-xs leading-5 text-black/70">
                {bubble.actionRawInput}
              </code>
            </div>
          )}
        </div>
      ) : (
        <div className={bodyShell}>
          <div className="whitespace-pre-wrap break-words text-sm text-black">
            {bubble.text}
          </div>
        </div>
      )}
      {!disabled && (
        <div className="flex flex-wrap gap-2 px-3 pb-3">
          {choices.map((c) => {
            const isDecline = /reject|decline|cancel|abort|no/i.test(c.label);
            const kind = isQuestionRequest(bubble.requestType)
              ? "answered"
              : isDecline
                ? "declined"
                : "accepted";
            return (
              <button
                key={c.id}
                onClick={() => void respond(c.id, kind)}
                className={clsx(
                  "btn-brutal-sm px-3 py-1 text-xs font-black",
                  isDecline
                    ? "bg-white"
                    : "bg-brutal-pink",
                )}
              >
                {c.label}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

function actionStatusTone(status: NonNullable<BubbleT["actionStatus"]>) {
  if (status === "answered") {
    return {
      label: "Answered",
      icon: CheckCircle2,
      iconClass: "text-black",
      card: "bg-brutal-yellow",
      badge: "bg-white text-black",
    };
  }
  if (status === "accepted") {
    return {
      label: "Approved",
      icon: CheckCircle2,
      iconClass: "text-black",
      card: "bg-brutal-lime",
      badge: "bg-white text-black",
    };
  }
  if (status === "declined") {
    return {
      label: "Rejected",
      icon: XCircle,
      iconClass: "text-black",
      card: "bg-danger",
      badge: "bg-white text-black",
    };
  }
  return {
    label: "Waiting",
    icon: Clock3,
    iconClass: "text-black",
    card: "bg-brutal-yellow",
    badge: "bg-white text-black",
  };
}

function isQuestionRequest(requestType?: string): boolean {
  return requestType === "question" || requestType === "human_decision";
}
