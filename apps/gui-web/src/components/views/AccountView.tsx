import { Avatar } from "@/components/layout/Avatar";
import { PageHeader } from "@/components/shared/PageComponents";
import { SettingsSection } from "@/components/views/SettingsView";
import { Button } from "@/components/ui/button";
import { accountName, capitalize } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Github, LogOut } from "lucide-react";
import type { HumanAccount } from "@/ipc/types";
import * as ipc from "@/ipc/bridge";

export function AccountView({
  account,
  busy,
  onLogin,
  onLogout,
}: {
  account: HumanAccount | null;
  busy: string | null;
  onLogin: (provider: ipc.LoginProvider) => void;
  onLogout: () => void;
}) {
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Account" detail="Identity and sign-in" />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 soft-scrollbar">
        <div className="mx-auto max-w-2xl">
          <SettingsSection title="Account" detail="Used for presence and local ownership.">
            {account ? (
              <div className="space-y-5">
                <div className="flex items-center gap-3">
                  <Avatar account={account} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-base font-bold text-[#111827]">
                      {accountName(account)}
                    </div>
                    <div className="truncate text-sm text-[#667085]">
                      {account.email || account.staffId || capitalize(account.provider)}
                    </div>
                  </div>
                  <Button
                    variant="outline"
                    onClick={onLogout}
                    disabled={busy === "logout"}
                  >
                    <LogOut size={15} />
                    Sign Out
                  </Button>
                </div>
                <div className="grid gap-3 sm:grid-cols-2">
                  <AccountField label="Provider" value={capitalize(account.provider)} />
                  <AccountField label="Actor ID" value={account.actorId} mono />
                  <AccountField label="Staff ID" value={account.staffId || "-"} />
                  <AccountField label="Email" value={account.email || "-"} />
                </div>
              </div>
            ) : (
              <div className="flex flex-wrap items-center gap-2">
                <Button onClick={() => onLogin("github")} disabled={busy === "login:github"}>
                  <Github size={15} />
                  GitHub
                </Button>
                <Button
                  variant="outline"
                  onClick={() => onLogin("google")}
                  disabled={busy === "login:google"}
                >
                  Google
                </Button>
              </div>
            )}
          </SettingsSection>
        </div>
      </div>
    </section>
  );
}


export function AccountField({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="min-w-0 rounded-lg border border-[#edf0f5] bg-[#fbfbfd] px-3 py-2">
      <div className="text-[11px] font-semibold uppercase tracking-wide text-[#667085]">
        {label}
      </div>
      <div
        className={cn(
          "mt-1 truncate text-sm font-semibold text-[#303849]",
          mono && "font-mono",
        )}
      >
        {value}
      </div>
    </div>
  );
}


