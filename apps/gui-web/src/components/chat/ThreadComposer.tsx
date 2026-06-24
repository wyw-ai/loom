import { useEffect, useRef, useState } from "react";
import type { Actor } from "@/ipc/types";
import type { MentionOption } from "@/lib/format-utils";
import { activeMentionQuery, mentionCandidates } from "@/lib/format-utils";
import { isComposingKeyEvent, shouldSendOnEnter } from "@/lib/format-utils";
import { Textarea } from "@/components/ui/textarea";
import { Button } from "@/components/ui/button";
import { Send, Loader2 } from "lucide-react";
import { MentionMenu } from "@/components/chat/MentionMenu";

export function ThreadComposer({
  draft,
  setDraft,
  disabled,
  busy,
  mentionAgents,
  onSend,
}: {
  draft: string;
  setDraft: (value: string) => void;
  disabled: boolean;
  busy: boolean;
  mentionAgents: Actor[];
  onSend: () => void;
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
    <footer className="shrink-0 border-t border-[#edf0f5] bg-white p-4">
      <div className="composer-box composer-box-compact relative">
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
          placeholder={disabled ? "Select a thread" : "Reply in thread..."}
          className="max-h-36 min-h-[42px] flex-1 border-0 bg-transparent px-0 py-1 text-sm shadow-none focus-visible:ring-0"
        />
        <Button
          size="icon"
          onClick={onSend}
          disabled={disabled || !draft.trim() || busy}
          className="h-9 w-9 rounded-lg bg-[#503ed4] text-white hover:bg-[#4635c5]"
        >
          {busy ? <Loader2 className="animate-spin" size={17} /> : <Send size={17} />}
        </Button>
      </div>
    </footer>
  );
}
