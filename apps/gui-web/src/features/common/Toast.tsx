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
        "pointer-events-none fixed bottom-6 left-1/2 z-[70] -translate-x-1/2 border-2 border-black px-4 py-2 text-sm font-bold shadow-brutal",
        toast.level === "error" && "bg-danger text-black",
        toast.level === "warn" && "bg-brutal-orange text-black",
        toast.level === "info" && "bg-brutal-yellow text-black",
      )}
      role="status"
    >
      {toast.message}
    </div>
  );
}
