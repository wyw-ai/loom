import { Avatar } from "@/components/layout/Avatar";
import { PageHeader } from "@/components/shared/PageComponents";
import { SettingsSection } from "@/components/settings/SettingsSection";
import { Button } from "@/components/ui/button";
import { avatarLibraryUrls } from "@/lib/constants";
import { accountName, capitalize } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { useEffect, useState } from "react";
import { Github, LogOut } from "lucide-react";
import type { HumanAccount } from "@/ipc/types";

export function AccountView({
  account,
  busy,
  onLogout,
  onAvatarChange,
}: {
  account: HumanAccount | null;
  busy: string | null;
  onLogout: () => void;
  onAvatarChange: (avatarUrl: string) => void;
}) {
  const [customAvatarUrl, setCustomAvatarUrl] = useState(account?.avatarUrl ?? "");
  useEffect(() => {
    setCustomAvatarUrl(account?.avatarUrl ?? "");
  }, [account?.avatarUrl]);
  const savingAvatar = busy === "account:avatar";

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
                      {account.email || account.staffId || providerLabel(account.provider)}
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
                  <AccountField label="Provider" value={providerLabel(account.provider)} />
                  <AccountField label="Actor ID" value={account.actorId} mono />
                  <AccountField label="User ID" value={account.staffId || "-"} />
                  <AccountField label="Email" value={account.email || "-"} />
                </div>
                <div className="space-y-3 rounded-lg border border-[#edf0f5] bg-[#fbfbfd] p-3">
                  <div>
                    <div className="text-sm font-bold text-[#303849]">Avatar</div>
                    <div className="text-xs text-[#667085]">
                      Used for presence and local ownership.
                    </div>
                  </div>
                  <div className="grid grid-cols-6 gap-2 sm:grid-cols-8">
                    {avatarLibraryUrls.map((url) => {
                      const selected = account.avatarUrl === url;
                      return (
                        <button
                          key={url}
                          type="button"
                          className={cn(
                            "h-10 w-10 rounded-full border bg-white p-0.5 transition",
                            selected ? "border-[#2563eb] ring-2 ring-[#bfdbfe]" : "border-[#dfe3ec]",
                          )}
                          onClick={() => onAvatarChange(url)}
                          disabled={savingAvatar}
                          title="Use this avatar"
                        >
                          <img
                            src={url}
                            alt=""
                            className="h-full w-full rounded-full object-cover"
                          />
                        </button>
                      );
                    })}
                  </div>
                  <div className="flex gap-2">
                    <input
                      className="min-w-0 flex-1 rounded-md border border-[#dfe3ec] bg-white px-3 py-2 text-sm outline-none focus:border-[#2563eb]"
                      value={customAvatarUrl}
                      onChange={(event) => setCustomAvatarUrl(event.target.value)}
                      placeholder="Custom avatar URL"
                      disabled={savingAvatar}
                    />
                    <Button
                      variant="outline"
                      disabled={savingAvatar}
                      onClick={() => onAvatarChange(customAvatarUrl.trim())}
                    >
                      Save
                    </Button>
                  </div>
                </div>
              </div>
            ) : (
              <div className="space-y-3">
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    disabled
                    title="GitHub login is not supported yet"
                  >
                    <Github size={15} />
                    GitHub not supported
                  </Button>
                  <Button
                    variant="outline"
                    disabled
                    title="Google login is not supported yet"
                  >
                    Google not supported
                  </Button>
                </div>
                <div className="rounded-lg border border-[#dfe3ec] bg-[#fbfbfd] px-3 py-2 text-sm text-[#667085]">
                  GitHub and Google sign-in are not available yet. Use Custom Identity for this build.
                </div>
              </div>
            )}
          </SettingsSection>
        </div>
      </div>
    </section>
  );
}

function providerLabel(provider: HumanAccount["provider"]) {
  if (provider === "local") return "Local";
  if (provider === "github") return "GitHub";
  if (provider === "google") return "Google";
  return capitalize(provider);
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

