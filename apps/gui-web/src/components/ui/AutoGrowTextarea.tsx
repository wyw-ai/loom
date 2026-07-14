import * as React from "react";
import TextareaAutosize from "react-textarea-autosize";
import { cn } from "@/lib/utils";

/**
 * Auto-growing textarea wrapper around react-textarea-autosize.
 *
 * In auto mode the textarea grows up to `maxRows` rows, then scrolls.
 * When a fixed height is set (manual mode), the textarea fills the
 * container and scrolls internally.
 */

export interface AutoGrowTextareaProps
  extends Omit<React.TextareaHTMLAttributes<HTMLTextAreaElement>, "style"> {
  /** Maximum rows before the textarea starts scrolling (auto mode). */
  maxRows?: number;
  /** Minimum rows. */
  minRows?: number;
  /** When set, the textarea uses this fixed height and scrolls internally. */
  fixedHeight?: number | null;
  onHeightChange?: (height: number) => void;
}

export const AutoGrowTextarea = React.forwardRef<
  HTMLTextAreaElement,
  AutoGrowTextareaProps
>(function AutoGrowTextarea(
  {
    className,
    maxRows = 5,
    minRows = 1,
    fixedHeight = null,
    onHeightChange,
    ...props
  },
  ref,
) {
  if (fixedHeight !== null && fixedHeight !== undefined) {
    return (
      <textarea
        ref={ref}
        className={cn(
          "flex w-full resize-none border-0 bg-transparent px-0 py-1 shadow-none focus-visible:outline-none focus-visible:ring-0",
          className,
        )}
        style={{ height: fixedHeight, overflowY: "auto" }}
        {...props}
      />
    );
  }

  return (
    <TextareaAutosize
      ref={ref}
      className={cn(
        "flex w-full resize-none border-0 bg-transparent px-0 py-1 shadow-none focus-visible:outline-none focus-visible:ring-0",
        className,
      )}
      maxRows={maxRows}
      minRows={minRows}
      onHeightChange={(height) => onHeightChange?.(height)}
      {...props}
    />
  );
});
