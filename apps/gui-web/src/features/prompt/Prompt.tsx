import { useEffect, useMemo, useRef, useState } from "react";
import {
  CornerDownRight,
  ImagePlus,
  Loader2,
  Paperclip,
  Send,
  X,
} from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { Actor, Channel, ScopeRef, Thread } from "@/ipc/types";
import { scopeKey } from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useChannels } from "@/store/channels";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { SlashPalette, type SlashPaletteHandle } from "./SlashPalette";
import { MentionPalette, type MentionPaletteHandle } from "./MentionPalette";
import { tryHandleSlash } from "./slashDispatch";

const IME_ENTER_GUARD_MS = 300;
const MAX_ATTACHMENT_BYTES = 16 * 1024 * 1024;
const MAX_TOTAL_ATTACHMENT_BYTES = 32 * 1024 * 1024;

type DraftRelation = {
  kind: "hands_off_to" | "replies_to" | "attaches_artifact";
  target: { kind: "actor" | "event" | "artifact"; id: string };
};

interface MentionTrigger {
  start: number;
  end: number;
  filter: string;
}

interface PendingAttachment {
  id: string;
  file: File;
  previewUrl?: string;
}

export function Prompt({ scope }: { scope: ScopeRef }) {
  const currentScopeKey = scopeKey(scope);
  const selfId = useSession((s) => s.workspace?.actorId);
  const drafts = useUI((s) => s.drafts);
  const setDraft = useUI((s) => s.setDraft);
  const actorsById = useActors((s) => s.byId);
  const channels = useChannels((s) => s.channels);
  const threadsByChannel = useChannels((s) => s.threadsByChannel);
  const reply = useUI((s) => s.replyTargets[currentScopeKey] ?? null);
  const replyAuthor = useActors((s) =>
    reply ? s.byId[reply.actorId] : undefined,
  );
  const setReply = useUI((s) => s.setReplyTarget);
  const pushToast = useUI((s) => s.pushToast);

  const text = drafts[currentScopeKey] ?? "";
  const [sending, setSending] = useState(false);
  const [caret, setCaret] = useState(0);
  const [asTask, setAsTask] = useState(false);
  const [pendingAttachments, setPendingAttachments] = useState<PendingAttachment[]>([]);
  const pendingAttachmentsRef = useRef<PendingAttachment[]>([]);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const imageInputRef = useRef<HTMLInputElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const slashRef = useRef<SlashPaletteHandle>(null);
  const mentionRef = useRef<MentionPaletteHandle>(null);
  const composingRef = useRef(false);
  const ignoreEnterUntilRef = useRef(0);

  const trimmed = text.trimStart();
  const slashOpen = trimmed.startsWith("/") && !trimmed.includes(" ");
  const mentionTrigger = useMemo(
    () => findMentionTrigger(text, Math.min(caret, text.length)),
    [text, caret],
  );
  const atOpen = mentionTrigger !== null;

  const placeholder = useMemo(
    () =>
      scope.kind === "channel"
        ? `Message in #${scope.id}`
        : `Message this thread`,
    [scope],
  );

  useEffect(() => {
    const onFocusPrompt = (event: Event) => {
      const detail = (event as CustomEvent<{ scopeKey?: string }>).detail;
      if (detail?.scopeKey !== currentScopeKey) return;
      taRef.current?.focus();
    };
    window.addEventListener("joi:focus-prompt", onFocusPrompt);
    return () => window.removeEventListener("joi:focus-prompt", onFocusPrompt);
  }, [currentScopeKey]);

  useEffect(() => {
    pendingAttachmentsRef.current = pendingAttachments;
  }, [pendingAttachments]);

  useEffect(() => {
    return () => {
      for (const attachment of pendingAttachmentsRef.current) {
        if (attachment.previewUrl) URL.revokeObjectURL(attachment.previewUrl);
      }
    };
  }, []);

  const send = async () => {
    if (!selfId) return;
    const body = text.trim();
    if (!body && pendingAttachments.length === 0) return;
    setSending(true);
    try {
      // Dispatch slash verbs other than /handoff (which is handled inline
      // below so its body becomes a real content.add handoff event).
      if (
        pendingAttachments.length === 0 &&
        body.startsWith("/") &&
        !body.startsWith("/handoff")
      ) {
        const result = await tryHandleSlash(body, { scope, actorId: selfId });
        if (result === "consumed") {
          setDraft(scope, "");
          return;
        }
      }

      // `/handoff @x msg` shortcut — keep parity with TUI slash command.
      let payloadText = body;
      const relations: DraftRelation[] = [];
      const handoffTargets = new Set<string>();
      const addHandoff = (actorId: string) => {
        if (handoffTargets.has(actorId)) return;
        handoffTargets.add(actorId);
        relations.push({
          kind: "hands_off_to",
          target: { kind: "actor", id: actorId },
        });
      };

      const handoff = body.match(/^\/handoff\s+@(\S+)\s*(.*)$/s);
      const atMention = body.match(/^@(\S+)\s+(.+)$/s);
      if (handoff) {
        addHandoff(handoff[1]);
        payloadText = handoff[2];
      } else if (atMention) {
        addHandoff(atMention[1]);
        payloadText = atMention[2];
      }

      for (const actorId of mentionTargets(payloadText, actorsById)) {
        if (actorId !== selfId && actorId !== "system") addHandoff(actorId);
      }

      if (reply?.eventId) {
        relations.push({
          kind: "replies_to",
          target: { kind: "event", id: reply.eventId },
        });
        // Mirror TUI `message_relations`: when replying to someone who
        // isn't self or "system", attach `hands_off_to` so the target
        // agent actually wakes up. Skip if the user already put their own
        // @mention in the text — don't double up.
        const replyHandoffActor = reply.handoffTarget ?? reply.actorId;
        if (
          replyHandoffActor &&
          replyHandoffActor !== selfId &&
          replyHandoffActor !== "system" &&
          handoffTargets.size === 0
        ) {
          addHandoff(replyHandoffActor);
        }
      }

      const blockedHandoffs = privateChannelHandoffBlockers({
        scope,
        channels,
        threadsByChannel,
        actorsById,
        actorIds: [...handoffTargets],
      });
      if (blockedHandoffs.length > 0) {
        pushToast("warn", blockedHandoffs[0]);
        return;
      }

      const artifactIds = await publishPendingAttachments({
        actorId: selfId,
        scope,
        attachments: pendingAttachments,
      });
      for (const artifactId of artifactIds) {
        relations.push({
          kind: "attaches_artifact",
          target: { kind: "artifact", id: artifactId },
        });
      }

      await ipc.eventAppend(
        asTask
          ? {
              type: "action.request",
              actorId: selfId,
              scope,
              payload: {
                requestType: "task",
                title: firstLine(payloadText),
                description: payloadText,
                choices: [
                  { id: "done", label: "Done" },
                  { id: "cancel", label: "Cancel" },
                ],
              },
              relations,
            }
          : {
              type: "content.add",
              actorId: selfId,
              scope,
              payload: { contentType: "text/markdown", text: payloadText },
              relations,
            },
      );
      setDraft(scope, "");
      clearPendingAttachments();
      setAsTask(false);
      setReply(scope, null);
    } catch (e) {
      pushToast("error", `send failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSending(false);
    }
  };

  const updateCaret = (el: HTMLTextAreaElement) => {
    setCaret(el.selectionStart ?? el.value.length);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const nativeEvent = e.nativeEvent;
    const nativeComposing =
      nativeEvent.isComposing || nativeEvent.keyCode === 229;
    const isImeEnter =
      composingRef.current ||
      nativeComposing ||
      Date.now() < ignoreEnterUntilRef.current;

    if (e.key === "Enter" && !e.shiftKey) {
      if (isImeEnter) {
        e.preventDefault();
        return;
      }
      // If a palette is open, Enter commits the first match instead of
      // sending. This matches Discord and fixes the "@agent<Enter> sent
      // the literal text" bug.
      if (slashOpen && slashRef.current?.pickFirst()) {
        e.preventDefault();
        return;
      }
      if (atOpen && mentionRef.current?.pickFirst()) {
        e.preventDefault();
        return;
      }
      e.preventDefault();
      void send();
      return;
    }
    if (e.key === "Escape") {
      if (reply) {
        setReply(scope, null);
        e.preventDefault();
      }
    }
  };

  const onCompositionStart = () => {
    composingRef.current = true;
    ignoreEnterUntilRef.current = 0;
  };

  const onCompositionEnd = () => {
    composingRef.current = false;
    // Some IMEs clear `isComposing` before the Enter keydown that confirms
    // the candidate/raw English text. Keep a short guard for that key event.
    ignoreEnterUntilRef.current = Date.now() + IME_ENTER_GUARD_MS;
  };

  const replaceLeadingToken = (token: string) => {
    const rest = text.replace(/^\s*[@/]\S*/, "");
    const next = `${token}${rest.startsWith(" ") ? "" : " "}${rest}`;
    setDraft(scope, next);
    taRef.current?.focus();
  };

  const replaceMentionToken = (actorId: string) => {
    const active = mentionTrigger;
    if (!active) {
      replaceLeadingToken(`@${actorId}`);
      return;
    }
    const token = `@${actorId}`;
    const before = text.slice(0, active.start);
    const after = text.slice(active.end);
    const spacer = after.length === 0 || !/^\s/.test(after) ? " " : "";
    const next = `${before}${token}${spacer}${after}`;
    const nextCaret = before.length + token.length + spacer.length;
    setDraft(scope, next);
    window.requestAnimationFrame(() => {
      taRef.current?.focus();
      taRef.current?.setSelectionRange(nextCaret, nextCaret);
      setCaret(nextCaret);
    });
  };

  const addPendingFiles = (files: File[]) => {
    if (files.length === 0) return;
    const currentTotal = pendingAttachments.reduce(
      (sum, attachment) => sum + attachment.file.size,
      0,
    );
    let nextTotal = currentTotal;
    const accepted: PendingAttachment[] = [];

    for (const file of files) {
      if (file.size > MAX_ATTACHMENT_BYTES) {
        pushToast(
          "warn",
          `${file.name} exceeds ${formatBytes(MAX_ATTACHMENT_BYTES)}`,
        );
        continue;
      }
      if (nextTotal + file.size > MAX_TOTAL_ATTACHMENT_BYTES) {
        pushToast(
          "warn",
          `attachments exceed ${formatBytes(MAX_TOTAL_ATTACHMENT_BYTES)}`,
        );
        continue;
      }
      nextTotal += file.size;
      accepted.push({
        id: crypto.randomUUID(),
        file,
        previewUrl: isImageFile(file)
          ? URL.createObjectURL(file)
          : undefined,
      });
    }

    if (accepted.length > 0) {
      setPendingAttachments((current) => [...current, ...accepted]);
    }
  };

  const removePendingAttachment = (id: string) => {
    setPendingAttachments((current) => {
      const target = current.find((attachment) => attachment.id === id);
      if (target?.previewUrl) URL.revokeObjectURL(target.previewUrl);
      return current.filter((attachment) => attachment.id !== id);
    });
  };

  const clearPendingAttachments = () => {
    setPendingAttachments((current) => {
      for (const attachment of current) {
        if (attachment.previewUrl) URL.revokeObjectURL(attachment.previewUrl);
      }
      return [];
    });
  };

  return (
    <div className="relative min-w-0 shrink-0 border-t-2 border-black bg-white px-3 py-3">
      {slashOpen && (
        <SlashPalette
          ref={slashRef}
          filter={trimmed.slice(1)}
          onPick={(cmd) => replaceLeadingToken(cmd)}
        />
      )}
      {atOpen && (
        <MentionPalette
          ref={mentionRef}
          filter={mentionTrigger.filter}
          onPick={replaceMentionToken}
        />
      )}

      {reply && (
        <div className="mb-2 flex items-center gap-2 border-2 border-black bg-brutal-cream px-3 py-1 text-xs font-bold text-black/70 shadow-brutal-sm">
          <CornerDownRight size={12} className="text-black/45" />
          <span className="truncate">
            Replying to{" "}
            <span className="font-semibold text-primary">
              {replyAuthor?.displayName || reply.actorId}
            </span>
            : {reply.preview}
          </span>
          <button
            className="ml-auto text-black/45 hover:text-danger"
            onClick={() => setReply(scope, null)}
          >
            <X size={12} />
          </button>
        </div>
      )}

      <div className="flex items-end gap-2 border-2 border-black bg-white px-3 py-2 shadow-brutal-sm">
        <textarea
          ref={taRef}
          value={text}
          placeholder={placeholder}
          onChange={(e) => {
            setDraft(scope, e.target.value);
            updateCaret(e.target);
          }}
          onSelect={(e) => updateCaret(e.currentTarget)}
          onClick={(e) => updateCaret(e.currentTarget)}
          onKeyUp={(e) => updateCaret(e.currentTarget)}
          onCompositionStart={onCompositionStart}
          onCompositionEnd={onCompositionEnd}
          onKeyDown={onKeyDown}
          rows={Math.min(10, Math.max(3, text.split("\n").length + 1))}
          className="min-h-[44px] flex-1 resize-none bg-transparent text-sm leading-6 text-black outline-none placeholder:text-black/40"
          disabled={sending}
        />
      </div>
      {pendingAttachments.length > 0 && (
        <div className="mt-2 grid gap-2">
          {pendingAttachments.map((attachment) => (
            <div
              key={attachment.id}
              className="flex min-w-0 items-center gap-3 border-2 border-black bg-brutal-cream px-3 py-2 shadow-brutal-sm"
            >
              {attachment.previewUrl ? (
                <img
                  src={attachment.previewUrl}
                  alt=""
                  className="h-10 w-10 shrink-0 border-2 border-black bg-white object-cover"
                />
              ) : (
                <div className="flex h-10 w-10 shrink-0 items-center justify-center border-2 border-black bg-white">
                  <Paperclip size={16} />
                </div>
              )}
              <div className="min-w-0 flex-1">
                <div className="truncate text-xs font-black text-black">
                  {attachment.file.name}
                </div>
                <div className="mt-0.5 font-mono text-[11px] text-black/45">
                  {guessMediaType(attachment.file)} ·{" "}
                  {formatBytes(attachment.file.size)}
                </div>
              </div>
              <button
                type="button"
                title="Remove attachment"
                onClick={() => removePendingAttachment(attachment.id)}
                className="btn-brutal-sm bg-white p-1"
                disabled={sending}
              >
                <X size={13} />
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="mt-2 flex items-center gap-2">
        <input
          ref={imageInputRef}
          type="file"
          accept="image/*"
          multiple
          className="hidden"
          onChange={(e) => {
            addPendingFiles(Array.from(e.currentTarget.files ?? []));
            e.currentTarget.value = "";
          }}
        />
        <input
          ref={fileInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={(e) => {
            addPendingFiles(Array.from(e.currentTarget.files ?? []));
            e.currentTarget.value = "";
          }}
        />
        <button
          type="button"
          className="btn-brutal-sm bg-white p-1"
          title="Attach image"
          aria-label="Attach image"
          disabled={sending}
          onClick={() => imageInputRef.current?.click()}
        >
          <ImagePlus size={15} />
        </button>
        <button
          type="button"
          className="btn-brutal-sm bg-white p-1"
          title="Attach file"
          aria-label="Attach file"
          disabled={sending}
          onClick={() => fileInputRef.current?.click()}
        >
          <Paperclip size={15} />
        </button>
        <label className="ml-auto flex items-center gap-1.5 text-xs font-bold text-black/70">
          <input
            type="checkbox"
            checked={asTask}
            onChange={(e) => setAsTask(e.target.checked)}
            className="h-3.5 w-3.5 accent-black"
          />
          As Task
        </label>
        <button
          type="button"
          disabled={(!text.trim() && pendingAttachments.length === 0) || sending}
          onClick={() => void send()}
          className="btn-brutal-sm gap-1 bg-brutal-pink px-3 text-xs disabled:bg-black/10"
        >
          {sending && pendingAttachments.length > 0 ? (
            <Loader2 size={13} className="animate-spin" />
          ) : (
            <Send size={13} />
          )}
          Send
        </button>
      </div>
    </div>
  );
}

function findMentionTrigger(text: string, caret: number): MentionTrigger | null {
  const before = text.slice(0, caret);
  const match = before.match(/@([A-Za-z0-9._-]*)$/);
  if (!match) return null;
  const filter = match[1] ?? "";
  const start = before.length - filter.length - 1;
  if (isMentionWordChar(before[start - 1])) return null;
  return {
    start,
    end: caret,
    filter,
  };
}

function mentionTargets(
  text: string,
  actorsById: Record<string, Actor>,
): string[] {
  const targets = new Set<string>();
  const re = /@([A-Za-z0-9._-]+)/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(text)) !== null) {
    if (isMentionWordChar(text[match.index - 1])) continue;
    const actorId = match[1];
    if (actorId && actorsById[actorId]) targets.add(actorId);
  }
  return [...targets];
}

function firstLine(value: string): string {
  return value.split("\n").find(Boolean) ?? "Untitled task";
}

function isMentionWordChar(ch: string | undefined): boolean {
  return !!ch && /[A-Za-z0-9._-]/.test(ch);
}

function privateChannelHandoffBlockers({
  scope,
  channels,
  threadsByChannel,
  actorsById,
  actorIds,
}: {
  scope: ScopeRef;
  channels: Channel[];
  threadsByChannel: Record<string, Thread[]>;
  actorsById: Record<string, Actor>;
  actorIds: string[];
}): string[] {
  const channel = resolveChannelForScope(scope, channels, threadsByChannel);
  if (!channel || channel.visibility !== "private") return [];

  const members = new Set(channel.members);
  return actorIds
    .filter((actorId) => !members.has(actorId))
    .map((actorId) => {
      const display = actorsById[actorId]?.displayName || actorId;
      return `${display} is not in #${channel.title}. Invite them before handing off.`;
    });
}

function resolveChannelForScope(
  scope: ScopeRef,
  channels: Channel[],
  threadsByChannel: Record<string, Thread[]>,
): Channel | null {
  if (scope.kind === "channel") {
    return channels.find((c) => c.id === scope.id) ?? null;
  }
  for (const [channelId, threads] of Object.entries(threadsByChannel)) {
    if (threads.some((thread) => thread.id === scope.id)) {
      return channels.find((c) => c.id === channelId) ?? null;
    }
  }
  return null;
}

async function publishPendingAttachments({
  actorId,
  scope,
  attachments,
}: {
  actorId: string;
  scope: ScopeRef;
  attachments: PendingAttachment[];
}): Promise<string[]> {
  const artifactIds: string[] = [];
  for (const attachment of attachments) {
    const bytes = await fileToBytes(attachment.file);
    const res = await ipc.artifactPublish({
      createdBy: actorId,
      scope,
      ingress: {
        kind: "file_bytes",
        name: attachment.file.name || "attachment",
        mediaType: guessMediaType(attachment.file),
        bytes,
      },
    });
    artifactIds.push(res.artifact.id);
  }
  return artifactIds;
}

async function fileToBytes(file: File): Promise<number[]> {
  return Array.from(new Uint8Array(await file.arrayBuffer()));
}

function isImageFile(file: File): boolean {
  return guessMediaType(file).startsWith("image/");
}

function guessMediaType(file: File): string {
  const explicit = file.type.trim();
  if (explicit) return explicit;
  const ext = file.name.split(".").pop()?.toLowerCase();
  switch (ext) {
    case "png":
      return "image/png";
    case "jpg":
    case "jpeg":
      return "image/jpeg";
    case "gif":
      return "image/gif";
    case "webp":
      return "image/webp";
    case "svg":
      return "image/svg+xml";
    case "pdf":
      return "application/pdf";
    case "md":
    case "markdown":
      return "text/markdown";
    case "json":
      return "application/json";
    case "yaml":
    case "yml":
      return "application/yaml";
    case "csv":
      return "text/csv";
    case "html":
    case "htm":
      return "text/html";
    case "css":
      return "text/css";
    case "js":
    case "mjs":
    case "cjs":
      return "text/javascript";
    case "txt":
    case "log":
    case "toml":
    case "ini":
    case "rs":
    case "go":
    case "py":
    case "ts":
    case "tsx":
    case "jsx":
    case "xml":
      return "text/plain";
    default:
      return "application/octet-stream";
  }
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 1024) {
    return `${Math.max(0, Math.round(bytes))} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}
