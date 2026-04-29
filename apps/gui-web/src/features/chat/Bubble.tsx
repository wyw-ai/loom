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
        className="my-1 scroll-mt-16 text-center text-xs italic text-muted"
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
      className="group relative flex scroll-mt-16 gap-3 rounded-md px-2 py-1 transition-colors hover:bg-hover focus-within:bg-hover"
      onContextMenu={onContextMenu}
    >
      <div className="w-10 shrink-0">
        {showHeader && (
          <button
            type="button"
            onClick={() => void revealId()}
            title={actorId}
            className="block rounded-full outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            <Avatar actorId={actorId} displayName={displayName} />
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
                "text-sm font-semibold outline-none hover:underline focus-visible:underline",
                isSelf ? "text-primary" : roleColor(actorId),
              )}
            >
              {displayName}
            </button>
            <span className="text-[11px] text-muted">
              {formatChatTime(bubble.ts)}
            </span>
            {bubble.delivery === "pending" && (
              <span className="text-[11px] text-muted">sending…</span>
            )}
          </header>
        )}

        {replyContext && (
          <button
            type="button"
            disabled={replyContext.missing}
            className="mb-0.5 flex max-w-full items-center gap-1.5 rounded-sm text-left text-xs text-muted hover:text-secondary disabled:cursor-default disabled:hover:text-muted"
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
      </div>

      {/* Hover toolbar — floats just above the top-right, Discord-style.
          It mirrors the context menu for the fast reply/copy path. */}
      <div
        className="pointer-events-none absolute -top-3 right-3 flex items-center gap-0.5 rounded-md border border-border bg-elevated px-0.5 py-0.5 opacity-0 shadow-md transition-opacity duration-75 group-hover:pointer-events-auto group-hover:opacity-100"
      >
        <button
          type="button"
          title="Reply"
          onClick={replyToThis}
          className="flex h-6 w-6 items-center justify-center rounded text-secondary hover:bg-hover hover:text-primary"
        >
          <CornerDownRight size={14} />
        </button>
        <button
          type="button"
          title="Copy text"
          onClick={() => void copyText()}
          className="flex h-6 w-6 items-center justify-center rounded text-secondary hover:bg-hover hover:text-primary"
        >
          <Copy size={14} />
        </button>
        <button
          type="button"
          title="Copy actor id"
          onClick={() => void revealId()}
          className="flex h-6 w-6 items-center justify-center rounded text-secondary hover:bg-hover hover:text-primary"
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

function Avatar({
  actorId,
  displayName,
}: {
  actorId: string;
  displayName: string;
}) {
  const base = displayName && displayName !== actorId ? displayName : actorId;
  const initials = base.slice(0, 2).toUpperCase();
  return (
    <div
      className="flex h-9 w-9 items-center justify-center rounded-full bg-elevated text-xs font-semibold text-secondary"
      aria-label={actorId}
    >
      {initials}
    </div>
  );
}

function roleColor(actorId: string): string {
  if (actorId.startsWith("actor_agent_")) return "text-role-agent";
  if (actorId.startsWith("actor_service_")) return "text-role-service";
  if (actorId === "system") return "text-muted";
  return "text-role-human";
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
    <div className="text-sm text-secondary">
      <span className="text-muted">→ handoff →</span>{" "}
      <span className="text-role-agent" title={target}>
        @{targetName}
      </span>
      {bubble.text ? <>: {bubble.text}</> : null}
    </div>
  );
}

function MarkdownText({ text }: { text: string }) {
  return (
    <div className="prose-chat text-sm leading-6 text-primary">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  );
}

function ActionRequestBody({ bubble }: { bubble: BubbleT }) {
  const currentScope = useChannels((s) => s.currentScope);
  const selfId = useSession((s) => s.workspace?.actorId);
  const pushToast = useUI((s) => s.pushToast);
  const disabled = bubble.acknowledged === true;

  const respond = async (optionId: string, kind: "accepted" | "declined") => {
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
  const [expanded, setExpanded] = useState(() => !disabled);
  const cardShell = "mt-1 w-full max-w-3xl overflow-hidden rounded-md border shadow-sm";
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
            className="flex h-5 w-5 items-center justify-center rounded text-muted hover:bg-hover hover:text-primary"
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
      <div className="min-w-0 truncate text-sm font-semibold text-primary">
        {actionTitle}
      </div>
      <span
        className={clsx(
          "shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium",
          statusTone.badge,
        )}
      >
        {statusTone.label}
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
            <div className="text-sm text-secondary">
              <div className="mb-0.5 text-[11px] font-semibold uppercase text-muted">
                Reason
              </div>
              <div className="whitespace-pre-wrap">{bubble.actionReason}</div>
            </div>
          )}
          {bubble.actionCommand && (
            <div>
              <div className="mb-1 text-[11px] font-semibold uppercase text-muted">
                Command
              </div>
              <code className="block overflow-x-auto rounded bg-elevated/80 px-2 py-1.5 font-mono text-xs leading-5 text-secondary">
                {bubble.actionCommand}
              </code>
            </div>
          )}
          {bubble.actionRawInput && (
            <div>
              <div className="mb-1 text-[11px] font-semibold uppercase text-muted">
                Raw input
              </div>
              <code className="block overflow-x-auto whitespace-pre rounded bg-elevated/80 px-2 py-1.5 font-mono text-xs leading-5 text-secondary">
                {bubble.actionRawInput}
              </code>
            </div>
          )}
        </div>
      ) : (
        <div className={bodyShell}>
          <div className="text-sm text-primary whitespace-pre-wrap">
            {bubble.text}
          </div>
        </div>
      )}
      {!disabled && (
        <div className="flex flex-wrap gap-2 px-3 pb-3">
          {choices.map((c) => {
            const isDecline = /reject|decline|cancel|abort|no/i.test(c.label);
            return (
              <button
                key={c.id}
                onClick={() =>
                  void respond(c.id, isDecline ? "declined" : "accepted")
                }
                className={clsx(
                  "rounded px-3 py-1 text-xs font-medium",
                  isDecline
                    ? "bg-elevated text-secondary hover:bg-hover"
                    : "bg-accent text-accent-contrast hover:bg-accent-hover",
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
  if (status === "accepted") {
    return {
      label: "Approved",
      icon: CheckCircle2,
      iconClass: "text-success",
      card: "border-[rgba(59,165,93,0.35)] bg-[rgba(59,165,93,0.12)]",
      badge: "bg-[rgba(59,165,93,0.16)] text-success",
    };
  }
  if (status === "declined") {
    return {
      label: "Rejected",
      icon: XCircle,
      iconClass: "text-danger",
      card: "border-[rgba(237,66,69,0.35)] bg-[rgba(237,66,69,0.12)]",
      badge: "bg-[rgba(237,66,69,0.16)] text-danger",
    };
  }
  return {
    label: "Waiting",
    icon: Clock3,
    iconClass: "text-warning",
    card: "border-[rgba(242,177,74,0.35)] bg-[rgba(242,177,74,0.12)]",
    badge: "bg-[rgba(242,177,74,0.16)] text-warning",
  };
}
