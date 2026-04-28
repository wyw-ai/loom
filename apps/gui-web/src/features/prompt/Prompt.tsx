import { useEffect, useMemo, useRef, useState } from "react";
import { CornerDownRight, X } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { ScopeRef } from "@/ipc/types";
import { scopeKey } from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";
import { SlashPalette, type SlashPaletteHandle } from "./SlashPalette";
import { MentionPalette, type MentionPaletteHandle } from "./MentionPalette";
import { tryHandleSlash } from "./slashDispatch";

const IME_ENTER_GUARD_MS = 80;

export function Prompt({ scope }: { scope: ScopeRef }) {
  const currentScopeKey = scopeKey(scope);
  const selfId = useSession((s) => s.workspace?.actorId);
  const drafts = useUI((s) => s.drafts);
  const setDraft = useUI((s) => s.setDraft);
  const reply = useUI((s) => s.replyTargets[currentScopeKey] ?? null);
  const replyAuthor = useActors((s) =>
    reply ? s.byId[reply.actorId] : undefined,
  );
  const setReply = useUI((s) => s.setReplyTarget);
  const pushToast = useUI((s) => s.pushToast);

  const text = drafts[currentScopeKey] ?? "";
  const [sending, setSending] = useState(false);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const slashRef = useRef<SlashPaletteHandle>(null);
  const mentionRef = useRef<MentionPaletteHandle>(null);
  const composingRef = useRef(false);
  const ignoreEnterUntilRef = useRef(0);

  const trimmed = text.trimStart();
  const slashOpen = trimmed.startsWith("/") && !trimmed.includes(" ");
  const atOpen = trimmed.startsWith("@") && !trimmed.includes(" ");

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

  const send = async () => {
    if (!selfId) return;
    const body = text.trim();
    if (!body) return;
    setSending(true);
    try {
      // Dispatch slash verbs other than /handoff (which is handled inline
      // below so its body becomes a real content.add handoff event).
      if (body.startsWith("/") && !body.startsWith("/handoff")) {
        const result = await tryHandleSlash(body, { scope, actorId: selfId });
        if (result === "consumed") {
          setDraft(scope, "");
          return;
        }
      }

      // `/handoff @x msg` shortcut — keep parity with TUI slash command.
      let payloadText = body;
      const relations: Array<{
        kind: "hands_off_to" | "replies_to";
        target: { kind: "actor" | "event"; id: string };
      }> = [];

      const handoff = body.match(/^\/handoff\s+@(\S+)\s*(.*)$/s);
      const atMention = body.match(/^@(\S+)\s+(.+)$/s);
      if (handoff) {
        relations.push({
          kind: "hands_off_to",
          target: { kind: "actor", id: handoff[1] },
        });
        payloadText = handoff[2];
      } else if (atMention) {
        relations.push({
          kind: "hands_off_to",
          target: { kind: "actor", id: atMention[1] },
        });
        payloadText = atMention[2];
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
        const alreadyHandsOff = relations.some(
          (r) => r.kind === "hands_off_to",
        );
        const replyHandoffActor = reply.handoffTarget ?? reply.actorId;
        if (
          !alreadyHandsOff &&
          replyHandoffActor &&
          replyHandoffActor !== selfId &&
          replyHandoffActor !== "system"
        ) {
          relations.push({
            kind: "hands_off_to",
            target: { kind: "actor", id: replyHandoffActor },
          });
        }
      }

      await ipc.eventAppend({
        type: "content.add",
        actorId: selfId,
        scope,
        payload: { contentType: "text/markdown", text: payloadText },
        relations,
      });
      setDraft(scope, "");
      setReply(scope, null);
    } catch (e) {
      pushToast("error", `send failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSending(false);
    }
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
        if (!composingRef.current && !nativeComposing) {
          e.preventDefault();
        }
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

  return (
    <div className="relative border-t border-border bg-main px-4 py-3">
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
          filter={trimmed.slice(1)}
          onPick={(actorId) => replaceLeadingToken(`@${actorId}`)}
        />
      )}

      {reply && (
        <div className="mb-2 flex items-center gap-2 rounded bg-elevated px-3 py-1 text-xs text-secondary">
          <CornerDownRight size={12} className="text-muted" />
          <span className="truncate">
            Replying to{" "}
            <span className="font-semibold text-primary">
              {replyAuthor?.displayName || reply.actorId}
            </span>
            : {reply.preview}
          </span>
          <button
            className="ml-auto text-muted hover:text-danger"
            onClick={() => setReply(scope, null)}
          >
            <X size={12} />
          </button>
        </div>
      )}

      <div className="flex items-end gap-2 rounded bg-elevated px-3 py-2">
        <textarea
          ref={taRef}
          value={text}
          placeholder={placeholder}
          onChange={(e) => setDraft(scope, e.target.value)}
          onCompositionStart={onCompositionStart}
          onCompositionEnd={onCompositionEnd}
          onKeyDown={onKeyDown}
          rows={Math.min(10, Math.max(3, text.split("\n").length + 1))}
          className="flex-1 resize-none bg-transparent text-sm leading-6 outline-none placeholder:text-muted"
          disabled={sending}
        />
      </div>
      <div className="mt-1 px-1 text-[11px] text-muted">
        Enter to send · Shift+Enter for newline · / for commands · @ for mentions
      </div>
    </div>
  );
}
