import { useState } from "react";
import { WifiOff } from "lucide-react";

import { useSession } from "@/store/session";
import { connectWorkspace } from "@/features/workspaces/connect";

export function DisconnectedOverlay() {
  const workspace = useSession((s) => s.workspace);
  const [retrying, setRetrying] = useState(false);

  const retry = async () => {
    if (!workspace || retrying) return;
    setRetrying(true);
    try {
      await connectWorkspace(workspace.id);
    } finally {
      setRetrying(false);
    }
  };

  return (
    <div className="pointer-events-auto absolute inset-0 z-40 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="flex w-[360px] max-w-[80%] flex-col items-center gap-3 rounded-lg border border-border bg-elevated p-6 text-center shadow-[0_8px_24px_rgba(0,0,0,.45)]">
        <WifiOff size={28} className="text-danger" />
        <h2 className="text-base font-semibold text-primary">Disconnected</h2>
        <p className="text-sm text-secondary">
          Lost connection to {workspace?.name ?? "the server"}.
        </p>
        <button
          onClick={retry}
          disabled={retrying}
          className="mt-1 rounded bg-accent px-4 py-1.5 text-sm text-accent-contrast hover:bg-accent-hover disabled:opacity-60"
        >
          {retrying ? "Retrying…" : "Retry"}
        </button>
      </div>
    </div>
  );
}
