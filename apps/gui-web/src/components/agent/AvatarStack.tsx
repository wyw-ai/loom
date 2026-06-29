import { cn } from "@/lib/utils";
import { actorAvatarUrl } from "@/lib/agent-utils";
import { displayName } from "@/lib/format-utils";
import type { Actor } from "@/ipc/types";

export function AvatarStack({
  actors,
  max,
  small,
}: {
  actors: Actor[];
  max: number;
  small?: boolean;
}) {
  const visible = actors.slice(0, max);
  const overflow = Math.max(0, actors.length - visible.length);
  return (
    <div className="flex items-center">
      {visible.map((actor, index) => (
        <img
          key={`${actor.id}:${index}`}
          alt=""
          src={actorAvatarUrl(actor, actor.id)}
          className={cn(
            "-ml-2 rounded-full border-2 border-white object-cover shadow-sm first:ml-0",
            small ? "h-6 w-6" : "h-8 w-8",
          )}
          title={displayName(actor)}
        />
      ))}
      {overflow > 0 && (
        <span
          className={cn(
            "-ml-2 inline-flex items-center justify-center rounded-full border-2 border-white bg-[#f1efff] text-[10px] font-bold text-[#5843d7]",
            small ? "h-6 min-w-6 px-1" : "h-8 min-w-8 px-1.5",
          )}
        >
          +{overflow}
        </span>
      )}
    </div>
  );
}
