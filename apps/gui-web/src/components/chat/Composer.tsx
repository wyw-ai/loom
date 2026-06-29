import { useEffect, useRef, useState } from "react";
import type { Actor, Message } from "@/ipc/types";
import type { MentionOption } from "@/lib/format-utils";
import { activeMentionQuery, mentionCandidates } from "@/lib/format-utils";
import { isComposingKeyEvent, shouldSendOnEnter } from "@/lib/format-utils";
import { Textarea } from "@/components/ui/textarea";
import { Button } from "@/components/ui/button";
import { Send, Loader2, X } from "lucide-react";
import { MentionMenu } from "@/components/chat/MentionMenu";

export function Composer({
  draft,
  setDraft,
  disabled,
  replyTo,
  actorName,
  onClearReply,
  onSend,
  mentionAgents,
  placeholder = "Message",
  disabledPlaceholder = "Connect and select a channel",
  busy,
}: {
  draft: string;
  setDraft: (value: string) => void;
  disabled: boolean;
  replyTo: Message | null;
  actorName: string;
  onClearReply: () => void;
  onSend: () => void;
  mentionAgents: Actor[];
  placeholder?: string;
  disabledPlaceholder?: string;
  busy: boolean;
}) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [caretIndex, setCaretIndex] = useState(draft.length);
  const [selectedMentionIndex, setSelectedMentionIndex] = useState(0);
  const [dismissedMentionKey, setDismissedMentionKey] = useState<string | null>(null);
  const activeMention = activeMentionQuery(draft, caretIndex);
  const mentionKey = activeMention
    ? `${activeMention.start}:${activeMention.end}:${activeMention.query}`
    : null;
  const mentionOptions = activeMention
    ? mentionCandidates(mentionAgents, activeMention)
    : [];
  const showMentions =
    !disabled &&
    !busy &&
    activeMention !== null &&
    dismissedMentionKey !== mentionKey &&
    mentionOptions.length > 0;
  const effectiveMentionIndex = mentionOptions.length
    ? Math.min(selectedMentionIndex, mentionOptions.length - 1)
    : 0;
  const selectedMention = showMentions ? mentionOptions[effectiveMentionIndex] : null;

  useEffect(() => {
    setSelectedMentionIndex(0);
  }, [mentionKey]);

  function syncCaret(element: HTMLTextAreaElement) {
    setCaretIndex(element.selectionStart ?? element.value.length);
  }

  function chooseMention(option: MentionOption) {
    const before = draft.slice(0, option.start);
    const after = draft.slice(option.end).replace(/^\s*/, "");
    const next = `${before}${option.token} ${after}`;
    const nextCaret = before.length + option.token.length + 1;
    setDraft(next);
    setCaretIndex(nextCaret);
    setDismissedMentionKey(null);
    requestAnimationFrame(() => {
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(nextCaret, nextCaret);
    });
  }

  return (
    <footer className="border-t border-[#e2e6ef] bg-white px-5 py-4">
      <div className="mx-auto max-w-4xl">
        {replyTo && (
          <div className="mb-2 flex items-center gap-2 rounded-lg border border-[#dfe3ec] bg-[#f7f8fb] px-3 py-2 text-xs text-[#667085]">
            <span className="min-w-0 flex-1 truncate">Replying to {actorName}</span>
            <button onClick={onClearReply}>
              <X size={14} />
            </button>
          </div>
        )}
        <div className="composer-box relative">
          {showMentions && (
            <MentionMenu
              options={mentionOptions}
              selectedIndex={effectiveMentionIndex}
              onSelect={chooseMention}
            />
          )}
          <Textarea
            ref={textareaRef}
            value={draft}
            onChange={(event) => {
              setDraft(event.target.value);
              syncCaret(event.currentTarget);
              setDismissedMentionKey(null);
            }}
            onClick={(event) => syncCaret(event.currentTarget)}
            onKeyUp={(event) => syncCaret(event.currentTarget)}
            onKeyDown={(event) => {
              if (isComposingKeyEvent(event)) return;
              if (showMentions) {
                if (event.key === "ArrowDown") {
                  event.preventDefault();
                  setSelectedMentionIndex((index) =>
                    (index + 1) % mentionOptions.length,
                  );
                  return;
                }
                if (event.key === "ArrowUp") {
                  event.preventDefault();
                  setSelectedMentionIndex((index) =>
                    (index - 1 + mentionOptions.length) % mentionOptions.length,
                  );
                  return;
                }
                if ((event.key === "Enter" || event.key === "Tab") && selectedMention) {
                  event.preventDefault();
                  chooseMention(selectedMention);
                  return;
                }
                if (event.key === "Escape") {
                  event.preventDefault();
                  setDismissedMentionKey(mentionKey);
                  return;
                }
              }
              if (shouldSendOnEnter(event)) {
                event.preventDefault();
                onSend();
              }
            }}
            disabled={disabled}
            placeholder={disabled ? disabledPlaceholder : placeholder}
            className="max-h-48 min-h-[44px] flex-1 border-0 bg-transparent px-0 py-1 shadow-none focus-visible:ring-0"
          />
          <Button
            size="icon"
            onClick={onSend}
            disabled={disabled || !draft.trim() || busy}
            className="h-9 w-9 rounded-lg"
          >
            {busy ? <Loader2 className="animate-spin" size={17} /> : <Send size={17} />}
          </Button>
        </div>
      </div>
    </footer>
  );
}
