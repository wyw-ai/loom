import { Badge } from "@/components/ui/badge";
import { taskStatusBadgeClass } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import type { Task } from "@/ipc/types";

export function TaskStateBadge({ task }: { task: Task }) {
  return (
    <Badge
      variant="outline"
      title={task.id}
      className={cn("whitespace-nowrap font-semibold", taskStatusBadgeClass(task.status))}
    >
      Task #{task.number} · {task.status}
    </Badge>
  );
}
