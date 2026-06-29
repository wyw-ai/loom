import type { Actor } from "@/ipc/types";
import { actorAvatarUrl } from "@/lib/agent-utils";
import { cn } from "@/lib/utils";

export function ActorAvatar({
  actor,
  fallback,
  small,
}: {
  actor?: Actor;
  fallback: string;
  small?: boolean;
}) {
  const src = actorAvatarUrl(actor, fallback);
  return (
    <img
      alt=""
      src={src}
      className={cn(
        "shrink-0 rounded-xl border border-white object-cover shadow-sm",
        small ? "h-7 w-7" : "h-10 w-10",
      )}
      title={actor?.id ?? fallback}
    />
  );
}
