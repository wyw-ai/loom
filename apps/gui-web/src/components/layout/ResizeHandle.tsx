import type { PointerEvent } from "react";
import { cn } from "@/lib/utils";

export function ResizeHandle({
  active,
  className,
  label,
  onKeyboardResize,
  onPointerDown,
}: {
  active: boolean;
  className?: string;
  label: string;
  onKeyboardResize: (delta: number) => void;
  onPointerDown: (event: PointerEvent<HTMLButtonElement>) => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      className={cn("resize-handle", active && "resize-handle-active", className)}
      onKeyDown={(event) => {
        const step = event.shiftKey ? 32 : 16;
        if (event.key === "ArrowLeft") {
          event.preventDefault();
          onKeyboardResize(-step);
        } else if (event.key === "ArrowRight") {
          event.preventDefault();
          onKeyboardResize(step);
        }
      }}
      onPointerDown={onPointerDown}
    />
  );
}
