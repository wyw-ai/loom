import { useEffect, useRef, useState } from "react";
import { Resizable } from "re-resizable";
import type { Actor } from "@/ipc/types";
import type { MentionOption } from "@/lib/format-utils";
import { activeMentionQuery, mentionCandidates } from "@/lib/format-utils";
import { isComposingKeyEvent, shouldSendOnEnter } from "@/lib/format-utils";
import { COMPOSER_AUTO_MAX_ROWS, THREAD_COMPOSER_MIN_HEIGHT } from "@/lib/composer-utils";
import { useComposerResize } from "@/hooks/useComposerResize";
import { useAttachments } from "@/hooks/useAttachments";
import { shouldWarnLongText, LONG_TEXT_THRESHOLD } from "@/lib/attachment-utils";
import { AutoGrowTextarea } from "@/components/ui/AutoGrowTextarea";
import { Button } from "@/components/ui/button";
import { Send, Loader2, Paperclip, AlertTriangle } from "lucide-react";
import { MentionMenu } from "@/components/chat/MentionMenu";
import { ComposerResizeHandle } from "@/components/chat/ComposerResizeHandle";
import { AttachmentPreviewBar } from "@/components/chat/AttachmentPreviewBar";

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
  onSend: (attachments?: import("@/lib/attachment-utils").PendingAttachment[]) => void;
}) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [caretIndex, setCaretIndex] = useState(draft.length);
  const [selectedMentionIndex, setSelectedMentionIndex] = useState(0);
  const [dismissedMentionKey, setDismissedMentionKey] = useState<string | null>(null);
  const [dragHover, setDragHover] = useState(false);
  const {
    attachments,
    error: attachmentError,
    fileInputRef,
    addFiles,
    removeAttachment,
    clearAttachments,
    openFilePicker,
  } = useAttachments();

  // --- Resize state ---
  const {
    effectiveHeight,
    isManual,
    liveHeight,
    maxHeightPx,
    handleResize,
    handleResizeStop,
    handleKeyboardResize,
    handleReset,
  } = useComposerResize({ type: "thread", maxHeightOverride: 300 });
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

  function handleSend() {
    onSend(attachments.length > 0 ? attachments : undefined);
    clearAttachments();
  }

  const showLongTextWarning = shouldWarnLongText(draft.length);
  const willConvertLongText = draft.length >= LONG_TEXT_THRESHOLD;

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
      <Resizable
        className="w-full"
        enable={{ top: true, right: false, bottom: false, left: false, topRight: false, bottomRight: false, bottomLeft: false, topLeft: false }}
        size={{ width: "100%", height: effectiveHeight }}
        minHeight={THREAD_COMPOSER_MIN_HEIGHT}
        maxHeight={maxHeightPx}
        onResizeStop={handleResizeStop}
        onResize={handleResize}
        handleStyles={{
          top: {
            top: "-4px",
            height: "6px",
            width: "100%",
            position: "absolute",
            cursor: "row-resize",
          },
        }}
        handleComponent={{
          top: (
            <ComposerResizeHandle
              onKeyboardResize={handleKeyboardResize}
              onReset={handleReset}
              ariaLabel="Resize thread reply input. Use arrow keys to adjust height, Enter to reset."
            />
          ),
        }}
      >
        <div className="flex h-full flex-col">
          <AttachmentPreviewBar attachments={attachments} onRemove={removeAttachment} />
          {(showLongTextWarning || willConvertLongText) && (
            <div className="mb-1.5 flex shrink-0 items-center gap-1.5 rounded-lg border border-amber-200 bg-amber-50 px-3 py-1 text-xs font-medium text-amber-700">
              <AlertTriangle size={12} className="shrink-0" />
              {willConvertLongText
                ? `Message exceeds ${LONG_TEXT_THRESHOLD} characters and will be sent as a .txt attachment.`
                : `Message is approaching the ${LONG_TEXT_THRESHOLD} character limit.`}
            </div>
          )}
          {attachmentError && (
            <div className="mb-1.5 shrink-0 rounded-lg border border-red-200 bg-red-50 px-3 py-1 text-xs font-medium text-red-700">
              {attachmentError}
            </div>
          )}
          <input
            ref={fileInputRef as React.RefObject<HTMLInputElement>}
            type="file"
            multiple
            className="hidden"
            onChange={(e) => {
              if (e.target.files) addFiles(e.target.files);
              e.target.value = "";
            }}
          />
          <div
            className={`composer-box composer-box-compact relative flex-1${isManual ? " composer-box-manual" : ""}${dragHover ? " ring-2 ring-blue-400" : ""}`}
            onDrop={(e) => {
              e.preventDefault();
              setDragHover(false);
              if (e.dataTransfer.files.length > 0) {
                addFiles(e.dataTransfer.files);
              }
            }}
            onDragOver={(e) => {
              e.preventDefault();
              setDragHover(true);
            }}
            onDragLeave={() => setDragHover(false)}
          >
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
              fixedHeight={(isManual || liveHeight !== null) ? effectiveHeight - 20 : null}
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
                  handleSend();
                }
              }}
              disabled={disabled}
              placeholder={disabled ? "Select a thread" : "Reply in thread..."}
              className="max-h-full min-h-[42px] w-full flex-1 px-3 mr-12 text-sm"
            />
            <Button
              size="icon"
              onClick={handleSend}
              disabled={disabled || (!draft.trim() && attachments.length === 0) || busy}
              className="absolute bottom-2 right-3 h-9 w-9 shrink-0 rounded-lg bg-[#503ed4] text-white hover:bg-[#4635c5]"
            >
              {busy ? <Loader2 className="animate-spin" size={17} /> : <Send size={17} />}
            </Button>
            <button
              type="button"
              onClick={openFilePicker}
              disabled={disabled}
              title="Attach files"
              className="absolute bottom-[46px] right-3 z-10 flex h-9 w-9 items-center justify-center rounded-lg text-[#667085] transition-colors hover:bg-[#f0f2f7] hover:text-[#1d2939] disabled:cursor-not-allowed disabled:opacity-40"
            >
              <Paperclip size={16} />
            </button>
          </div>
        </div>
      </Resizable>
    </footer>
  );
}
