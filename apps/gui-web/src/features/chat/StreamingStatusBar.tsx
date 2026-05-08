import { X } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import type { ScopeRef } from "@/ipc/types";
import { scopeKey } from "@/ipc/types";
import { useActors } from "@/store/actors";
import { useMessages } from "@/store/messages";
import { useUI } from "@/store/ui";

export function StreamingStatusBar({ scope }: { scope: ScopeRef }) {
  const openTurns = useMessages(
    (s) => s.byScope[scopeKey(scope)]?.openTurns ?? {},
  );
  const actorsById = useActors((s) => s.byId);
  const pushToast = useUI((s) => s.pushToast);

  const entries = Object.entries(openTurns);
  if (entries.length === 0) return null;

  return (
    <div className="flex items-center gap-2 border-t-2 border-black bg-brutal-cream px-4 py-1 text-xs font-bold text-black/70">
      {entries.map(([turnId, info]) => (
        <span
          key={turnId}
          className="inline-flex items-center gap-1 border border-black bg-white px-2 py-0.5"
        >
          <span className="inline-block h-2 w-2 animate-pulse rounded-full border border-black bg-brutal-lime" />
          <span className="text-black" title={info.actorId}>
            @{actorsById[info.actorId]?.displayName || info.actorId}
          </span>
          <span>typing...</span>
          <button
            aria-label="Cancel turn"
            className="ml-1 text-black/45 hover:text-danger"
            onClick={async () => {
              try {
                await ipc.turnClose(turnId, "cancelled");
              } catch (e) {
                pushToast(
                  "error",
                  `cancel failed: ${e instanceof Error ? e.message : String(e)}`,
                );
              }
            }}
          >
            <X size={12} />
          </button>
        </span>
      ))}
    </div>
  );
}
