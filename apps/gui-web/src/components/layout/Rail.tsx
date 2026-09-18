import { useEffect, useId, useRef, useState } from "react";
import { Check, Loader2, Plus, Settings, UserRound, Users } from "lucide-react";
import type { HumanAccount, Workspace } from "@/ipc/types";
import type { ConnectionState } from "@/lib/types";
import { accountName, workspaceInitials } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Avatar } from "@/components/layout/Avatar";
import { usePresence } from "@/hooks/usePresence";
import { useI18n } from "@/lib/i18n";

export function Rail({
  account,
  busy,
  connection,
  workspace,
  workspaces,
  onSelectWorkspace,
  onOpenSpaces,
  onOpenAccount,
  onOpenSystemSettings,
  spacesActive,
  accountActive,
  systemSettingsActive,
}: {
  account: HumanAccount | null;
  busy: string | null;
  connection: ConnectionState;
  workspace: Workspace | null;
  workspaces: Workspace[];
  onSelectWorkspace: (workspaceId: string) => void;
  onOpenSpaces: () => void;
  onOpenAccount: () => void;
  onOpenSystemSettings: () => void;
  spacesActive: boolean;
  accountActive: boolean;
  systemSettingsActive: boolean;
}) {
  const { t } = useI18n();
  const [accountMenuOpen, setAccountMenuOpen] = useState(false);
  const accountMenuMounted = usePresence(accountMenuOpen);
  const accountMenuId = useId();
  const accountMenuRootRef = useRef<HTMLDivElement>(null);
  const accountMenuRef = useRef<HTMLDivElement>(null);
  const accountButtonRef = useRef<HTMLButtonElement>(null);
  const accountDestinationActive = accountActive || systemSettingsActive;

  useEffect(() => {
    if (!accountMenuOpen || !accountMenuMounted) return;

    accountMenuRef.current
      ?.querySelector<HTMLButtonElement>('[role="menuitem"]')
      ?.focus();

    const closeFromOutside = (event: globalThis.PointerEvent) => {
      if (!accountMenuRootRef.current?.contains(event.target as Node)) {
        setAccountMenuOpen(false);
      }
    };
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setAccountMenuOpen(false);
      accountButtonRef.current?.focus();
    };

    document.addEventListener("pointerdown", closeFromOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeFromOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [accountMenuMounted, accountMenuOpen]);

  useEffect(() => {
    setAccountMenuOpen(false);
  }, [workspace?.id]);

  const selectAccountDestination = (destination: "account" | "system") => {
    setAccountMenuOpen(false);
    if (destination === "account") onOpenAccount();
    else onOpenSystemSettings();
  };

  const handleMenuKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const items = Array.from(
      accountMenuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [],
    );
    if (!items.length) return;
    const currentIndex = items.indexOf(document.activeElement as HTMLButtonElement);
    const nextIndex = event.key === "Home"
      ? 0
      : event.key === "End"
        ? items.length - 1
        : event.key === "ArrowDown"
          ? (currentIndex + 1 + items.length) % items.length
          : (currentIndex - 1 + items.length) % items.length;
    items[nextIndex]?.focus();
  };

  return (
    <nav className="flex min-h-0 flex-col items-center border-r border-[#e2e6ef] bg-[#f7f8fb] px-2.5 py-4">
      <button
        type="button"
        title={t("Add or manage spaces")}
        aria-label={t("Add or manage spaces")}
        className={cn(
          "mb-4 flex h-11 w-11 items-center justify-center rounded-xl bg-gradient-to-br from-[#6f58f6] to-[#4b36d8] text-white shadow-sm ring-1 ring-white/60 transition-transform",
          spacesActive && "ring-2 ring-[#d9d4ff] scale-105",
        )}
        onClick={onOpenSpaces}
      >
        <Plus size={20} />
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
      </div>
      <div ref={accountMenuRootRef} className="relative flex flex-col items-center gap-3">
        <button
          ref={accountButtonRef}
          type="button"
          title={account
            ? t("{{name}} account", { name: accountName(account) })
            : t("Account")}
          aria-label={t("Open account menu")}
          aria-haspopup="menu"
          aria-expanded={accountMenuOpen}
          aria-controls={accountMenuOpen ? accountMenuId : undefined}
          className={cn(
            "relative rounded-xl outline-none transition focus-visible:ring-2 focus-visible:ring-[#8b7cf6] focus-visible:ring-offset-2",
            accountDestinationActive && "ring-2 ring-[#d9d4ff] ring-offset-2 ring-offset-[#f7f8fb]",
          )}
          onClick={() => setAccountMenuOpen((open) => !open)}
        >
          {account ? (
            <Avatar account={account} />
          ) : (
            <span className="flex h-10 w-10 items-center justify-center rounded-xl border border-[#dfe3ec] bg-white text-sm font-bold text-[#667085]">
              <Users size={17} />
            </span>
          )}
        </button>
        {accountMenuMounted ? (
          <div
            ref={accountMenuRef}
            id={accountMenuId}
            role={accountMenuOpen ? "menu" : undefined}
            aria-label={t("Account menu")}
            aria-hidden={!accountMenuOpen}
            className={cn(
              "surface-menu surface-menu-origin-bottom-left absolute bottom-0 left-[calc(100%+12px)] z-[70] w-52 rounded-xl border border-[#dfe3ec] bg-white p-1.5 shadow-[0_18px_48px_rgb(16_24_40_/_0.16)]",
              !accountMenuOpen && "motion-menu-closing pointer-events-none",
            )}
            onKeyDown={handleMenuKeyDown}
          >
            <AccountMenuItem
              active={accountActive}
              icon={UserRound}
              label={t("Personal Profile")}
              onClick={() => selectAccountDestination("account")}
            />
            <AccountMenuItem
              active={systemSettingsActive}
              icon={Settings}
              label={t("System Settings")}
              onClick={() => selectAccountDestination("system")}
            />
          </div>
        ) : null}
      </div>
    </nav>
  );
}

function AccountMenuItem({
  active,
  icon: Icon,
  label,
  onClick,
}: {
  active: boolean;
  icon: typeof UserRound;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm font-semibold outline-none transition-colors focus-visible:bg-[#f1efff] focus-visible:text-[#503ed4]",
        active
          ? "bg-[#f1efff] text-[#503ed4]"
          : "text-[#303849] hover:bg-[#f7f8fb]",
      )}
      onClick={onClick}
    >
      <Icon size={16} className="shrink-0" />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {active ? <Check size={15} aria-hidden="true" /> : null}
    </button>
  );
}
