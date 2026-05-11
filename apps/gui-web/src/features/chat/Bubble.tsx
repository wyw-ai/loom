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
import { PixelAvatar } from "@/features/common/PixelAvatar";

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
      className="my-1 scroll-mt-16 text-center font-mono text-xs italic text-black/40"
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
      className="group relative flex scroll-mt-16 gap-3 px-2 py-1 transition-colors hover:bg-brutal-cream focus-within:bg-brutal-cream"
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

function Avatar({
  actorId,
  displayName,
}: {
  actorId: string;
  displayName: string;
}) {
  return <PixelAvatar id={actorId} label={displayName} size={36} />;
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
    <div className="text-sm text-black/70">
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
    <div className="prose-chat text-sm leading-6 text-black">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  );
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
  const cardShell = "mt-1 w-full max-w-3xl overflow-hidden border-2 border-black bg-white shadow-brutal-sm";
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
              <div className="whitespace-pre-wrap">{bubble.actionReason}</div>
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
          <div className="whitespace-pre-wrap text-sm text-black">
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
