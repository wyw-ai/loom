import type { HumanAccount } from "@/ipc/types";
import { accountName, accountToActor } from "@/lib/format-utils";
import { actorAvatarUrl } from "@/lib/agent-utils";

export function Avatar({ account }: { account: HumanAccount }) {
  const actor = accountToActor(account);
  const src = actorAvatarUrl(actor, account.actorId || accountName(account));
  return (
    <img
      alt=""
      src={src}
      className="h-10 w-10 rounded-xl border border-white object-cover shadow-sm"
    />
  );
}
