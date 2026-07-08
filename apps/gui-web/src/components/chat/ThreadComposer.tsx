import { useCallback, useEffect, useRef, useState } from "react";
import { Resizable } from "re-resizable";
import type { Actor } from "@/ipc/types";
import type { MentionOption } from "@/lib/format-utils";
import { activeMentionQuery, mentionCandidates } from "@/lib/format-utils";
import { isComposingKeyEvent, shouldSendOnEnter } from "@/lib/format-utils";
import {
  COMPOSER_AUTO_MAX_ROWS,
  THREAD_COMPOSER_DEFAULT_CAP,
  THREAD_COMPOSER_MIN_HEIGHT,
  composerMaxHeightPx,
} from "@/lib/composer-utils";
import { useComposerStore } from "@/store/composerStore";
import { AutoGrowTextarea } from "@/components/ui/AutoGrowTextarea";
import { Button } from "@/components/ui/button";
import { Send, Loader2 } from "lucide-react";
import { MentionMenu } from "@/components/chat/MentionMenu";
import { ComposerResizeHandle } from "@/components/chat/ComposerResizeHandle";

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

  // --- Resize state ---
  const entry = useComposerStore((s) => s.entries.thread);
  const setManualHeight = useComposerStore((s) => s.setManualHeight);
  const resetToAuto = useComposerStore((s) => s.resetToAuto);
  const isManual = entry.mode === "manual" && entry.manualHeight !== null;
  const resolvedHeight = isManual ? entry.manualHeight! : THREAD_COMPOSER_DEFAULT_CAP;
  const maxHeightPx = Math.min(composerMaxHeightPx(), 300);

  const handleResizeStop = useCallback(
    (_e: MouseEvent | TouchEvent, _dir: string, _ref: HTMLElement, delta: { height: number }) => {
      const newHeight = Math.max(
        THREAD_COMPOSER_MIN_HEIGHT,
        Math.min(resolvedHeight + delta.height, maxHeightPx),
      );
      setManualHeight("thread", newHeight);
    },
    [resolvedHeight, maxHeightPx, setManualHeight],
  );

  const handleKeyboardResize = useCallback(
    (deltaPx: number) => {
      const newHeight = Math.max(
        THREAD_COMPOSER_MIN_HEIGHT,
        Math.min(resolvedHeight + deltaPx, maxHeightPx),
      );
      setManualHeight("thread", newHeight);
    },
    [resolvedHeight, maxHeightPx, setManualHeight],
  );

  const handleReset = useCallback(() => {
    resetToAuto("thread");
  }, [resetToAuto]);
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
    <footer className="relative shrink-0 border-t border-[#edf0f5] bg-white p-4">
      <ComposerResizeHandle
        onKeyboardResize={handleKeyboardResize}
        onReset={handleReset}
        ariaLabel="Resize thread reply input. Use arrow keys to adjust height, Enter to reset."
      />
      <Resizable
        className="w-full"
        enable={{ top: true, right: false, bottom: false, left: false, topRight: false, bottomRight: false, bottomLeft: false, topLeft: false }}
        size={{ width: "100%", height: resolvedHeight }}
        minHeight={THREAD_COMPOSER_MIN_HEIGHT}
        maxHeight={maxHeightPx}
        onResizeStop={handleResizeStop}
        handleStyles={{
          top: {
            cursor: "ns-resize",
            height: "8px",
            top: "-4px",
            width: "100%",
          },
        }}
      >
        <div className="composer-box composer-box-compact relative flex h-full flex-col">
          {showMentions && (
            <MentionMenu
              options={mentionOptions}
              selectedIndex={effectiveMentionIndex}
              onSelect={chooseMention}
            />
          )}
          <AutoGrowTextarea
            ref={textareaRef}
            value={draft}
            maxRows={COMPOSER_AUTO_MAX_ROWS}
            fixedHeight={isManual ? resolvedHeight - 32 : null}
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
            className="max-h-full min-h-[42px] flex-1 px-0 text-sm"
          />
          <Button
            size="icon"
            onClick={onSend}
            disabled={disabled || !draft.trim() || busy}
            className="h-9 w-9 shrink-0 rounded-lg bg-[#503ed4] text-white hover:bg-[#4635c5]"
          >
            {busy ? <Loader2 className="animate-spin" size={17} /> : <Send size={17} />}
          </Button>
        </div>
      </Resizable>
    </footer>
  );
}
