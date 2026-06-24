import type { HumanAccount } from "@/ipc/types";
import { accountName } from "@/lib/format-utils";
import { avatarUrlForSeed } from "@/lib/agent-utils";

export function Avatar({ account }: { account: HumanAccount }) {
  const src = account.avatarUrl || avatarUrlForSeed(account.actorId || accountName(account));
  return (
    <img
      alt=""
      src={src}
      className="h-10 w-10 rounded-xl border border-white object-cover shadow-sm"
    />
  );
}
