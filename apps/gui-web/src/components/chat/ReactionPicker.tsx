import { Smile } from "lucide-react";
import type { Message } from "@/ipc/types";
import { supportedReactionEmojis } from "@/lib/constants";
import { cn } from "@/lib/utils";

export function ReactionPicker({
  busy,
  compact = false,
  message,
  onToggleReaction,
}: {
  busy: string | null;
  compact?: boolean;
  message: Message;
  onToggleReaction: (message: Message, emoji: string) => void;
}) {
  return (
    <div className={cn("reaction-picker", compact && "h-7")}>
      <button
        type="button"
        className={cn(
          "composer-icon reaction-picker-trigger rounded-full",
          compact ? "h-7 min-w-7" : "h-8 min-w-8",
        )}
        title="Add reaction"
        aria-label="Add reaction"
        aria-haspopup="true"
      >
        <Smile size={compact ? 14 : 15} />
      </button>
      <div className="reaction-picker-menu" role="menu" aria-label="Choose reaction">
        {supportedReactionEmojis.map((emoji) => (
          <button
            key={emoji}
            type="button"
            className={cn("reaction-picker-option", compact && "h-7 w-7 text-sm")}
            title={`React ${emoji}`}
            aria-label={`React ${emoji}`}
            disabled={busy === `message:reaction:${message.id}:${emoji}`}
            onClick={() => onToggleReaction(message, emoji)}
            role="menuitem"
          >
            {emoji}
          </button>
        ))}
      </div>
    </div>
  );
}
