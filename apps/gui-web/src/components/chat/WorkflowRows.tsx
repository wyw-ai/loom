import { Check } from "lucide-react";
import type { Actor, Message, MachineInfo, Run } from "@/ipc/types";
import { Badge } from "@/components/ui/badge";
import { workflowSummary, workflowResultSummary } from "@/lib/message-utils";
import { formatTime } from "@/lib/utils";
import { displayName } from "@/lib/format-utils";
import { AgentMessageAvatar } from "@/components/agent/AgentMessageAvatar";

export function WorkflowEventRow({
  actor,
  actors,
  message,
}: {
  actor?: Actor;
  actors: Record<string, Actor>;
  message: Message;
}) {
  const summary = workflowSummary(message, actors);
  return (
    <div className="mx-auto flex max-w-[80%] items-center gap-2 rounded-xl border border-[#dfe3ec] bg-[#f7f8fb] px-3 py-2 text-xs text-[#667085]">
      <Check size={14} />
      <span className="min-w-0 flex-1 truncate">{summary}</span>
      <span>{formatTime(message.createdAt)}</span>
      {actor && <Badge variant="outline">{displayName(actor)}</Badge>}
    </div>
  );
}

export function WorkflowResultRow({
  actor,
  machines,
  runs,
  message,
  onOpenAgentSettings,
}: {
  actor?: Actor;
  machines: MachineInfo[];
  runs: Record<string, Run>;
  message: Message;
  onOpenAgentSettings: (actorId: string) => void;
}) {
  return (
    <article className="group rounded-xl px-4 py-3 transition-colors hover:bg-[#f7f8fb]">
      <div className="flex items-start gap-4">
        <AgentMessageAvatar
          actor={actor}
          fallback={message.authorActorId}
          machines={machines}
          runs={runs}
          onOpenAgentSettings={onOpenAgentSettings}
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-semibold text-[#111827]">{actor ? displayName(actor) : message.authorActorId}</span>
            <span className="text-xs text-muted-foreground">{formatTime(message.createdAt)}</span>
            <Badge variant="success">task result</Badge>
          </div>
          <div className="mt-1 text-sm leading-6 text-[#303849]">{workflowResultSummary(message)}</div>
        </div>
      </div>
    </article>
  );
}
