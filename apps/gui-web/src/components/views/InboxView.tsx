import { EmptyState } from "@/components/shared/EmptyState";
import { PageHeader } from "@/components/shared/PageComponents";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { actorName } from "@/lib/format-utils";
import { actionChoices, messageTitle } from "@/lib/message-utils";
import { shortId } from "@/lib/utils";
import { Bell } from "lucide-react";
import type { Actor, InboxListEntry, Message } from "@/ipc/types";

export function InboxView({
  actors,
  inbox,
  onOpen,
  onAnswer,
  busy,
}: {
  actors: Record<string, Actor>;
  inbox: InboxListEntry[];
  onOpen: (message: Message | null | undefined) => void;
  onAnswer: (message: Message, optionId: string, accepted: boolean) => void;
  busy: string | null;
}) {
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Inbox" detail={`${inbox.length} pending deliveries`} />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 scrollbar-thin">
        <div className="mx-auto max-w-3xl space-y-3">
          {inbox.length === 0 ? (
            <EmptyState icon={Bell} text="No pending deliveries." />
          ) : (
            inbox.map((item) => {
              const message = item.message;
              return (
                <div
                  key={item.delivery.sourceId}
                  className="rounded-md border border-border bg-card p-4"
                >
                  <div className="mb-2 flex items-center gap-2">
                    <Badge variant="warning">{item.delivery.state}</Badge>
                    <span className="font-mono text-xs text-muted-foreground">
                      {shortId(item.delivery.sourceId)}
                    </span>
                  </div>
                  {message ? (
                    <>
                      <div className="text-sm font-medium">
                        {messageTitle(message)}
                      </div>
                      <div className="mt-1 text-sm text-muted-foreground">
                        From {actorName(actors, message.authorActorId)}
                      </div>
                      <div className="mt-3 flex flex-wrap gap-2">
                        <Button variant="outline" size="sm" onClick={() => onOpen(message)}>
                          Open
                        </Button>
                        {actionChoices(message).map((choice) => (
                          <Button
                            key={choice.id}
                            size="sm"
                            variant={choice.accepted ? "default" : "outline"}
                            disabled={busy?.startsWith(`action:${message.id}:`)}
                            onClick={() => onAnswer(message, choice.id, choice.accepted)}
                          >
                            {choice.label}
                          </Button>
                        ))}
                      </div>
                    </>
                  ) : (
                    <div className="text-sm text-muted-foreground">Message unavailable.</div>
                  )}
                </div>
              );
            })
          )}
        </div>
      </div>
    </section>
  );
}


