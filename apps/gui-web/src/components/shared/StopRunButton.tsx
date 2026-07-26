import { useEffect, useRef, useState } from "react";
import { Loader2, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

const CONFIRM_RESET_MS = 4000;

/**
 * Two-step destructive-action button for run.cancel (the design requires a
 * 二次确认 before stopping a run). First click arms the confirm state, which
 * auto-resets after a timeout; second click fires `onConfirm`.
 */
export function StopRunButton({
  disabled = false,
  busy = false,
  compact = false,
  label = "Stop",
  confirmLabel = "Confirm stop?",
  title,
  className,
  onConfirm,
}: {
  disabled?: boolean;
  busy?: boolean;
  compact?: boolean;
  label?: string;
  confirmLabel?: string;
  title?: string;
  className?: string;
  onConfirm: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const timerRef = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    },
    [],
  );

  const disarm = () => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    setConfirming(false);
  };

  const handleClick = () => {
    if (disabled || busy) return;
    if (!confirming) {
      setConfirming(true);
      timerRef.current = window.setTimeout(() => {
        timerRef.current = null;
        setConfirming(false);
      }, CONFIRM_RESET_MS);
      return;
    }
    disarm();
    onConfirm();
  };

  if (compact) {
    return (
      <button
        type="button"
        title={title ?? label}
        aria-label={confirming ? confirmLabel : (title ?? label)}
        disabled={disabled || busy}
        onClick={handleClick}
        className={cn(
          "inline-flex h-5 shrink-0 items-center gap-1 rounded px-1.5 text-[11px] font-medium",
          confirming
            ? "bg-red-600 text-white hover:bg-red-700"
            : "text-[#667085] hover:bg-red-50 hover:text-red-700",
          className,
        )}
      >
        {busy ? (
          <Loader2 size={11} className="animate-spin" />
        ) : (
          <XCircle size={11} />
        )}
        {confirming ? "Stop?" : label}
      </button>
    );
  }

  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      title={title}
      disabled={disabled || busy}
      onClick={handleClick}
      className={cn(
        confirming &&
          "border-red-600 bg-red-600 text-white hover:bg-red-700 hover:text-white",
        className,
      )}
    >
      {busy ? (
        <Loader2 size={14} className="mr-1 animate-spin" />
      ) : (
        <XCircle size={14} className="mr-1" />
      )}
      {confirming ? confirmLabel : label}
    </Button>
  );
}
