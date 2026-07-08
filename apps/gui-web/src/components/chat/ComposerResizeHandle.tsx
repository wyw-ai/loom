import * as React from "react";
import { cn } from "@/lib/utils";

/**
 * Top-edge drag handle for the resizable composer.
 *
 * Accessibility:
 * - role="separator" with aria-orientation="horizontal"
 * - tabIndex=0 for keyboard focus
 * - Arrow Up/Down adjusts height by 8px
 * - Enter or double-click resets to auto mode
 * - aria-label in English
 */

export interface ComposerResizeHandleProps {
  /** Called when a keyboard arrow adjusts the height. `delta` is in pixels (negative = shrink). */
  onKeyboardResize: (deltaPx: number) => void;
  /** Called when the user presses Enter or double-clicks — should reset to auto. */
  onReset: () => void;
  className?: string;
  ariaLabel?: string;
}

export function ComposerResizeHandle({
  onKeyboardResize,
  onReset,
  className,
  ariaLabel = "Resize message input. Use arrow keys to adjust height, Enter to reset.",
}: ComposerResizeHandleProps) {
  const handleKeyDown = React.useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      switch (event.key) {
        case "ArrowUp":
          event.preventDefault();
          onKeyboardResize(8);
          break;
        case "ArrowDown":
          event.preventDefault();
          onKeyboardResize(-8);
          break;
        case "Enter":
          event.preventDefault();
          onReset();
          break;
      }
    },
    [onKeyboardResize, onReset],
  );

  return (
    <div
      role="separator"
      aria-orientation="horizontal"
      aria-label={ariaLabel}
      tabIndex={0}
      onKeyDown={handleKeyDown}
      onDoubleClick={onReset}
      className={cn(
        "group/handle flex h-[10px] cursor-ns-resize items-end justify-center outline-none",
        "focus-visible:ring-2 focus-visible:ring-[#503ed4] focus-visible:ring-offset-1",
        className,
      )}
    >
      <div className="mb-[-1px] h-1 w-10 rounded-full bg-[#d0d4de] transition-colors group-hover/handle:bg-[#503ed4]" />
    </div>
  );
}
