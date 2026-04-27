import { useEffect } from "react";
import clsx from "clsx";

import { useUI } from "@/store/ui";

export function Toast() {
  const toast = useUI((s) => s.toast);
  const clear = useUI((s) => s.clearToast);

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => clear(), 3000);
    return () => clearTimeout(t);
  }, [toast, clear]);

  if (!toast) return null;

  return (
    <div
      className={clsx(
        "pointer-events-none fixed bottom-6 left-1/2 -translate-x-1/2 rounded px-4 py-2 text-sm shadow-lg",
        "bg-elevated border border-border",
        toast.level === "error" && "text-danger",
        toast.level === "warn" && "text-warning",
        toast.level === "info" && "text-primary",
      )}
      role="status"
    >
      {toast.message}
    </div>
  );
}
