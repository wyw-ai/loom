import { EmptyState } from "@/components/shared/EmptyState";
import { PageHeader } from "@/components/shared/PageComponents";
import { Badge } from "@/components/ui/badge";
import { taskStatusBadgeClass } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Check } from "lucide-react";
import type { Channel, Task } from "@/ipc/types";

export function TasksView({
  tasks,
  channels,
}: {
  tasks: Task[];
  channels: Channel[];
}) {
  const channelsById = Object.fromEntries(channels.map((channel) => [channel.id, channel]));
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Tasks" detail={`${tasks.length} tasks`} />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 scrollbar-thin">
        <div className="mx-auto max-w-4xl space-y-2">
          {tasks.length === 0 ? (
            <EmptyState icon={Check} text="No tasks." />
          ) : (
            tasks.map((task) => (
              <div
                key={task.id}
                className="grid gap-3 rounded-md border border-border bg-card p-4 sm:grid-cols-[1fr_auto]"
              >
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant="secondary">#{task.number}</Badge>
                    <span className="truncate font-medium">{task.title}</span>
                  </div>
                  <div className="mt-1 line-clamp-2 text-sm text-muted-foreground">
                    {task.description || task.resultSummary || "No description."}
                  </div>
                </div>
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  <Badge
                    variant="outline"
                    className={cn("font-semibold", taskStatusBadgeClass(task.status))}
                  >
                    {task.status}
                  </Badge>
                  <span>{channelsById[task.channelId]?.title ?? task.channelId}</span>
                </div>
              </div>
            ))
          )}
        </div>
      </div>
    </section>
  );
}


