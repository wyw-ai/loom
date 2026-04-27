import { useEffect } from "react";
import { AlertTriangle, CheckCircle2, ExternalLink, XCircle } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import { useChannels } from "@/store/channels";
import { useInbox } from "@/store/inbox";
import { useMessages } from "@/store/messages";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";

export function InboxPage() {
  const items = useInbox((s) => s.items);
  const remove = useInbox((s) => s.remove);
  const markAllSeen = useInbox((s) => s.markAllSeen);
  const setScope = useChannels((s) => s.setCurrentScope);
  const ensureScope = useMessages((s) => s.ensureScope);
  const ingestBackfill = useMessages((s) => s.ingestBackfill);
  const setView = useUI((s) => s.setView);
  const pushToast = useUI((s) => s.pushToast);
  const selfId = useSession((s) => s.workspace?.actorId);

  useEffect(() => {
    markAllSeen();
  }, [markAllSeen]);

  const respond = async (
    eventId: string,
    scope: { kind: "channel" | "thread"; id: string },
    optionId: string,
    kind: "accepted" | "declined",
  ) => {
    if (!selfId) return;
    try {
      await ipc.eventAppend({
        type: "action.response",
        actorId: selfId,
        scope,
        payload: { optionId, kind },
        relations: [{ kind: "responds_to", target: { kind: "event", id: eventId } }],
      });
      remove(eventId);
    } catch (e) {
      pushToast(
        "error",
        `action.response failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  };

  const openScope = async (scope: { kind: "channel" | "thread"; id: string }) => {
    setScope(scope);
    ensureScope(scope);
    setView("chat");
    try {
      await ipc.scopeSubscribe(scope);
      const r = await ipc.scopeRead(scope, 100);
      ingestBackfill(scope, r.events);
    } catch {
      /* ignore */
    }
  };

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <header className="flex h-12 shrink-0 items-center border-b border-border px-6 text-sm font-semibold text-primary">
        Inbox
        <span className="ml-3 text-xs text-muted">
          Pending action.requests across all scopes
        </span>
      </header>

      <div className="stable-scrollbar min-h-0 flex-1 overflow-y-scroll px-6 py-4">
        {items.length === 0 ? (
          <div className="mt-10 text-center text-sm text-muted">
            You're caught up.
          </div>
        ) : (
          <ul className="space-y-3">
            {items.map((it) => (
              <li
                key={it.requestEventId}
                className="rounded border-l-4 border-warning bg-elevated px-4 py-3"
              >
                <div className="mb-1 flex items-center gap-2 text-xs text-muted">
                  <AlertTriangle size={14} className="text-warning" />
                  <span>
                    {it.scope.kind === "channel" ? "#" : "thread "}
                    {it.scope.id}
                  </span>
                  <span>·</span>
                  <span>{new Date(it.arrivedAt).toLocaleString()}</span>
                </div>
                <div className="mb-2 text-sm font-medium text-primary">
                  {it.title}
                </div>
                {it.reason || it.command ? (
                  <div className="mb-3 space-y-2">
                    {it.reason && (
                      <div className="text-sm text-secondary">
                        <div className="mb-0.5 text-[11px] font-semibold uppercase text-muted">
                          Reason
                        </div>
                        <div className="whitespace-pre-wrap">{it.reason}</div>
                      </div>
                    )}
                    {it.command && (
                      <div>
                        <div className="mb-1 text-[11px] font-semibold uppercase text-muted">
                          Command
                        </div>
                        <code className="block overflow-x-auto rounded bg-main px-2 py-1.5 font-mono text-xs leading-5 text-secondary">
                          {it.command}
                        </code>
                      </div>
                    )}
                  </div>
                ) : it.description ? (
                  <div className="mb-3 text-sm text-secondary whitespace-pre-wrap">
                    {it.description}
                  </div>
                ) : null}
                <div className="flex flex-wrap items-center gap-2">
                  {(it.choices.length > 0
                    ? it.choices
                    : [
                        { id: "approve", label: "Approve" },
                        { id: "reject", label: "Reject" },
                      ]
                  ).map((c) => {
                    const isDecline = /reject|decline|cancel|abort|no/i.test(c.label);
                    const Icon = isDecline ? XCircle : CheckCircle2;
                    return (
                      <button
                        key={c.id}
                        onClick={() =>
                          void respond(
                            it.requestEventId,
                            it.scope,
                            c.id,
                            isDecline ? "declined" : "accepted",
                          )
                        }
                        className={
                          isDecline
                            ? "inline-flex items-center gap-1 rounded bg-hover px-3 py-1 text-xs text-secondary hover:bg-border"
                            : "inline-flex items-center gap-1 rounded bg-accent px-3 py-1 text-xs text-accent-contrast hover:bg-accent-hover"
                        }
                      >
                        <Icon size={12} />
                        {c.label}
                      </button>
                    );
                  })}
                  <button
                    onClick={() => void openScope(it.scope)}
                    className="ml-auto inline-flex items-center gap-1 rounded px-3 py-1 text-xs text-secondary hover:text-primary"
                  >
                    <ExternalLink size={12} />
                    Open thread
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
