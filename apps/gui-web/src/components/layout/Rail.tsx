import { Loader2, Plus, Users } from "lucide-react";
import type { HumanAccount, Workspace } from "@/ipc/types";
import type { ConnectionState } from "@/lib/types";
import { accountName, workspaceInitials } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Avatar } from "@/components/layout/Avatar";

export function Rail({
  account,
  busy,
  connection,
  workspace,
  workspaces,
  onSelectWorkspace,
  onOpenHome,
  onOpenSpaces,
  onOpenAccount,
}: {
  account: HumanAccount | null;
  busy: string | null;
  connection: ConnectionState;
  workspace: Workspace | null;
  workspaces: Workspace[];
  onSelectWorkspace: (workspaceId: string) => void;
  onOpenHome: () => void;
  onOpenSpaces: () => void;
  onOpenAccount: () => void;
}) {
  return (
    <nav className="flex min-h-0 flex-col items-center border-r border-[#e2e6ef] bg-[#f7f8fb] px-2.5 py-4">
      <button
        type="button"
        title="Home"
        className="mb-4 flex h-11 w-11 items-center justify-center rounded-xl bg-gradient-to-br from-[#6f58f6] to-[#4b36d8] text-base font-bold text-white shadow-sm ring-1 ring-white/60"
        onClick={onOpenHome}
      >
        L
      </button>
      <div className="flex flex-1 flex-col items-center gap-2">
        {workspaces.map((item) => {
          const selected = item.id === workspace?.id;
          return (
            <button
              key={item.id}
              title={item.name}
              className={cn(
                "relative flex h-10 w-10 items-center justify-center rounded-xl border text-sm font-bold transition-colors",
                selected
                  ? "border-[#6e5bf2] bg-white text-[#5843d7] shadow-sm ring-2 ring-[#d9d4ff]"
                  : "border-[#dfe3ec] bg-white/70 text-[#303849] hover:border-[#c8cee0] hover:bg-white",
              )}
              onClick={() => onSelectWorkspace(item.id)}
              disabled={busy === `connect:${item.id}`}
            >
              {busy === `connect:${item.id}` ? (
                <Loader2 className="animate-spin" size={15} />
              ) : (
                workspaceInitials(item)
              )}
              {selected && connection === "open" && (
                <span className="absolute -bottom-0.5 -right-0.5 h-3.5 w-3.5 rounded-full border-2 border-[#f7f8fb] bg-emerald-400" />
              )}
            </button>
          );
        })}
        {workspaces.length === 0 && (
          <button
            type="button"
            title="Add space"
            className="flex h-10 w-10 items-center justify-center rounded-xl border border-[#dfe3ec] bg-white/70 text-[#667085] transition-colors hover:bg-white hover:text-[#5843d7]"
            onClick={onOpenSpaces}
          >
            <Plus size={18} />
          </button>
        )}
        {workspaces.length > 0 && (
          <button
            type="button"
            title="Manage spaces"
            className="mt-1 flex h-9 w-9 items-center justify-center rounded-xl border border-[#dfe3ec] bg-white/50 text-[#667085] transition-colors hover:bg-white hover:text-[#5843d7]"
            onClick={onOpenSpaces}
          >
            <Plus size={17} />
          </button>
        )}
      </div>
      <div className="flex flex-col items-center gap-3">
        <button
          type="button"
          title={account ? `${accountName(account)} account` : "Account"}
          className="relative"
          onClick={onOpenAccount}
        >
          {account ? (
            <Avatar account={account} />
          ) : (
            <span className="flex h-10 w-10 items-center justify-center rounded-xl border border-[#dfe3ec] bg-white text-sm font-bold text-[#667085]">
              <Users size={17} />
            </span>
          )}
        </button>
      </div>
    </nav>
  );
}
