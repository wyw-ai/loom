import { useEffect } from "react";
import { AlertTriangle, CheckCircle2, ExternalLink, XCircle } from "lucide-react";

import * as ipc from "@/ipc/bridge";
import { openScope as openChatScope } from "@/features/chat/scopeActions";
import { useInbox } from "@/store/inbox";
import { useSession } from "@/store/session";
import { useUI } from "@/store/ui";

export function InboxPage() {
  const items = useInbox((s) => s.items);
  const remove = useInbox((s) => s.remove);
  const markAllSeen = useInbox((s) => s.markAllSeen);
  const pushToast = useUI((s) => s.pushToast);
  const selfId = useSession((s) => s.workspace?.actorId);

  useEffect(() => {
    markAllSeen();
  }, [markAllSeen]);

  const respond = async (
    eventId: string,
    scope: { kind: "channel" | "thread"; id: string },
    optionId: string,
    kind: "accepted" | "declined" | "answered",
    actionRequestId?: string,
  ) => {
    if (!selfId) return;
    try {
      await openChatScope(scope);
      await ipc.eventAppend({
        type: "action.response",
        actorId: selfId,
        scope,
        payload: {
          optionId,
          kind,
          ...(actionRequestId ? { requestId: actionRequestId } : {}),
        },
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

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-white text-black">
      <header className="flex h-panel-header shrink-0 items-center border-b-2 border-black px-6 text-sm font-black text-black">
        Inbox
        <span className="ml-3 font-mono text-xs font-normal text-black/45">
          Pending action.requests across all scopes
        </span>
      </header>

      <div className="stable-scrollbar min-h-0 flex-1 overflow-y-scroll px-6 py-4">
        {items.length === 0 ? (
          <div className="mt-10 text-center font-mono text-sm text-black/40">
            You're caught up.
          </div>
        ) : (
          <ul className="space-y-3">
            {items.map((it) => (
              <li
                key={it.requestEventId}
                className="border-2 border-black bg-white px-4 py-3 shadow-brutal-sm"
              >
                <div className="mb-1 flex items-center gap-2 font-mono text-xs text-black/45">
                  <AlertTriangle size={14} className="text-black" />
                  <span>
                    {it.scope.kind === "channel" ? "#" : "thread "}
                    {it.scope.id}
                  </span>
                  <span>-</span>
                  <span>{new Date(it.arrivedAt).toLocaleString()}</span>
                </div>
                <div className="mb-2 text-sm font-black text-black">
                  {it.title}
                </div>
                {it.reason || it.command || it.rawInput ? (
                  <div className="mb-3 space-y-2">
                    {it.reason && (
                      <div className="text-sm text-black/70">
                        <div className="mb-0.5 text-[11px] font-black uppercase tracking-wider text-black/45">
                          Reason
                        </div>
                        <div className="whitespace-pre-wrap">{it.reason}</div>
                      </div>
                    )}
                    {it.command && (
                      <div>
                        <div className="mb-1 text-[11px] font-black uppercase tracking-wider text-black/45">
                          Command
                        </div>
                        <code className="block overflow-x-auto border-2 border-black bg-brutal-cream px-2 py-1.5 font-mono text-xs leading-5 text-black/70">
                          {it.command}
                        </code>
                      </div>
                    )}
                    {it.rawInput && (
                      <div>
                        <div className="mb-1 text-[11px] font-black uppercase tracking-wider text-black/45">
                          Raw input
                        </div>
                        <code className="block overflow-x-auto whitespace-pre border-2 border-black bg-brutal-cream px-2 py-1.5 font-mono text-xs leading-5 text-black/70">
                          {it.rawInput}
                        </code>
                      </div>
                    )}
                  </div>
                ) : it.description ? (
                  <div className="mb-3 whitespace-pre-wrap text-sm text-black/70">
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
                    const kind =
                      it.requestType === "question" ||
                      it.requestType === "human_decision"
                        ? "answered"
                        : isDecline
                          ? "declined"
                          : "accepted";
                    const Icon = isDecline ? XCircle : CheckCircle2;
                    return (
                      <button
                        key={c.id}
                        onClick={() =>
                          void respond(
                            it.requestEventId,
                            it.scope,
                            c.id,
                            kind,
                            it.actionRequestId,
                          )
                        }
                        className={
                          isDecline
                            ? "btn-brutal-sm inline-flex gap-1 bg-white px-3 py-1 text-xs"
                            : "btn-brutal-sm inline-flex gap-1 bg-brutal-pink px-3 py-1 text-xs"
                        }
                      >
                        <Icon size={12} />
                        {c.label}
                      </button>
                    );
                  })}
                  <button
                    onClick={() => void openChatScope(it.scope)}
                    className="btn-brutal-sm ml-auto inline-flex gap-1 bg-white px-3 py-1 text-xs"
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
