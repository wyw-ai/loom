import { Avatar } from "@/components/layout/Avatar";
import { PageHeader } from "@/components/shared/PageComponents";
import { SettingsSection } from "@/components/settings/SettingsSection";
import { Button } from "@/components/ui/button";
import { avatarLibraryUrls } from "@/lib/constants";
import { accountName, capitalize } from "@/lib/format-utils";
import {
  buildMobileConnectPayload,
  isLoopbackServerUrl,
  type MobileConnectMode,
} from "@/lib/mobile-connect";
import { cn } from "@/lib/utils";
import { useI18n } from "@/lib/i18n";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Github, LogOut, QrCode, Smartphone, X } from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
import type { HumanAccount, Workspace } from "@/ipc/types";

export function AccountView({
  account,
  workspace,
  busy,
  onLogout,
  onAvatarChange,
}: {
  account: HumanAccount | null;
  workspace: Workspace | null;
  busy: string | null;
  onLogout: () => void;
  onAvatarChange: (avatarUrl: string) => void;
}) {
  const { t } = useI18n();
  const [customAvatarUrl, setCustomAvatarUrl] = useState(account?.avatarUrl ?? "");
  const [mobileQrOpen, setMobileQrOpen] = useState(false);
  useEffect(() => {
    setCustomAvatarUrl(account?.avatarUrl ?? "");
  }, [account?.avatarUrl]);
  const savingAvatar = busy === "account:avatar";

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title={t("Personal Profile")} detail={t("Identity and sign-in")} />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 soft-scrollbar">
        <div className="mx-auto max-w-2xl">
          <SettingsSection title={t("Account")} detail={t("Used for presence and local ownership.")}>
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
                  <div className="flex items-center gap-2">
                    <Button
                      variant="outline"
                      size="icon"
                      aria-label={t("Connect Loom Mobile")}
                      title={workspace ? t("Connect Loom Mobile") : t("Connect to a space first")}
                      disabled={!workspace}
                      onClick={() => setMobileQrOpen(true)}
                    >
                      <QrCode size={17} />
                    </Button>
                    <Button
                      variant="outline"
                      onClick={onLogout}
                      disabled={busy === "logout"}
                    >
                      <LogOut size={15} />
                      {t("Sign Out")}
                    </Button>
                  </div>
                </div>
                <div className="grid gap-3 sm:grid-cols-2">
                  <AccountField label={t("Provider")} value={t(providerLabel(account.provider))} />
                  <AccountField label={t("Actor ID")} value={account.actorId} mono />
                  <AccountField label={t("User ID")} value={account.staffId || "-"} />
                  <AccountField label={t("Email")} value={account.email || "-"} />
                </div>
                <div className="space-y-3 rounded-lg border border-[#edf0f5] bg-[#fbfbfd] p-3">
                  <div>
                    <div className="text-sm font-bold text-[#303849]">{t("Avatar")}</div>
                    <div className="text-xs text-[#667085]">
                      {t("Used for presence and local ownership.")}
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
                          title={t("Use this avatar")}
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
                      placeholder={t("Custom avatar URL")}
                      disabled={savingAvatar}
                    />
                    <Button
                      variant="outline"
                      disabled={savingAvatar}
                      onClick={() => onAvatarChange(customAvatarUrl.trim())}
                    >
                      {t("Save")}
                    </Button>
                  </div>
                </div>
              </div>
            ) : (
              <div className="space-y-3">
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    disabled
                    title={t("GitHub login is not supported yet")}
                  >
                    <Github size={15} />
                    {t("GitHub not supported")}
                  </Button>
                  <Button
                    variant="outline"
                    disabled
                    title={t("Google login is not supported yet")}
                  >
                    {t("Google not supported")}
                  </Button>
                </div>
                <div className="rounded-lg border border-[#dfe3ec] bg-[#fbfbfd] px-3 py-2 text-sm text-[#667085]">
                  {t("GitHub and Google sign-in are not available yet. Use Custom Identity for this build.")}
                </div>
              </div>
            )}
          </SettingsSection>
        </div>
      </div>
      {mobileQrOpen && account && workspace ? (
        <MobileConnectDialog
          account={account}
          workspace={workspace}
          onClose={() => setMobileQrOpen(false)}
        />
      ) : null}
    </section>
  );
}

export function MobileConnectDialog({
  account,
  workspace,
  onClose,
}: {
  account: HumanAccount;
  workspace: Workspace;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [mode, setMode] = useState<MobileConnectMode>("self");
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);
  const connectionDisplayName = workspace.displayName.trim() || accountName(account);
  const payload = mode === "self"
    ? buildMobileConnectPayload({
        mode: "self",
        serverUrl: workspace.serverUrl,
        actorId: workspace.actorId,
        displayName: connectionDisplayName,
      })
    : buildMobileConnectPayload({
        mode: "invite",
        serverUrl: workspace.serverUrl,
      });
  const loopback = isLoopbackServerUrl(workspace.serverUrl);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const previouslyFocused = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    closeButtonRef.current?.focus();

    const handleDialogKeys = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;

      const focusable = Array.from(
        dialogRef.current?.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
      if (!focusable.length) {
        event.preventDefault();
        return;
      }
      const first = focusable[0]!;
      const last = focusable[focusable.length - 1]!;
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !dialogRef.current?.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || !dialogRef.current?.contains(active))) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleDialogKeys);
    return () => {
      document.removeEventListener("keydown", handleDialogKeys);
      if (previouslyFocused?.isConnected) previouslyFocused.focus();
    };
  }, []);

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[#111827]/35 px-4 py-6 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-labelledby="mobile-connect-title"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        className="flex w-full max-w-md flex-col rounded-2xl border border-[#dfe3ec] bg-white shadow-[0_28px_80px_rgb(16_24_40_/_0.22)]"
      >
        <div className="flex items-start justify-between gap-4 border-b border-[#edf0f5] px-5 py-4">
          <div className="min-w-0">
            <div
              id="mobile-connect-title"
              className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.16em] text-[#596174]"
            >
              <span className="flex h-7 w-7 items-center justify-center rounded-lg bg-[#f1efff] text-[#503ed4]">
                <Smartphone size={15} />
              </span>
              {t("Connect Loom Mobile")}
            </div>
            <div className="mt-2 text-sm text-[#667085]">
              {mode === "self"
                ? t("Scan in Loom Mobile to use this space and your current identity.")
                : t("Let someone else scan to join this space with their own identity.")}
            </div>
          </div>
          <button
            ref={closeButtonRef}
            type="button"
            className="composer-icon h-8 min-w-8"
            title={t("Close")}
            aria-label={t("Close mobile connection QR code")}
            onClick={onClose}
          >
            <X size={15} />
          </button>
        </div>

        <div className="space-y-4 p-5">
          <div
            className="grid grid-cols-2 gap-1 rounded-xl bg-[#f2f3f7] p-1"
            role="group"
            aria-label={t("Mobile QR code mode")}
          >
            <MobileConnectModeButton
              active={mode === "self"}
              onClick={() => setMode("self")}
            >
              {t("Join as yourself")}
            </MobileConnectModeButton>
            <MobileConnectModeButton
              active={mode === "invite"}
              onClick={() => setMode("invite")}
            >
              {t("Invite someone")}
            </MobileConnectModeButton>
          </div>

          <div className="mx-auto w-fit rounded-2xl border border-[#edf0f5] bg-white p-4 shadow-sm">
            <QRCodeSVG
              value={payload}
              size={224}
              level="M"
              marginSize={1}
              title={mode === "self" ? t("Loom Mobile self connection") : t("Loom Mobile invitation")}
            />
          </div>

          <div className="space-y-2 rounded-lg border border-[#edf0f5] bg-[#fbfbfd] p-3 text-sm">
            <div className="flex gap-3">
              <span className="w-16 shrink-0 text-[#667085]">{t("Space")}</span>
              <span className="min-w-0 truncate font-semibold text-[#303849]">{workspace.name}</span>
            </div>
            <div className="flex gap-3">
              <span className="w-16 shrink-0 text-[#667085]">{t("Server")}</span>
              <code className="min-w-0 break-all font-mono text-xs text-[#303849]">
                {workspace.serverUrl}
              </code>
            </div>
            {mode === "self" ? (
              <>
                <div className="flex gap-3">
                  <span className="w-16 shrink-0 text-[#667085]">{t("Identity")}</span>
                  <span className="min-w-0 truncate font-semibold text-[#303849]">
                    {connectionDisplayName}
                  </span>
                </div>
                <div className="flex gap-3">
                  <span className="w-16 shrink-0 text-[#667085]">{t("Actor ID")}</span>
                  <code className="min-w-0 break-all font-mono text-xs text-[#303849]">
                    {workspace.actorId}
                  </code>
                </div>
              </>
            ) : (
              <div className="rounded-md bg-[#eef4ff] px-3 py-2 text-xs leading-5 text-[#3448a3]">
                {t("This QR code includes only the server address. Your identity is not shared.")}
              </div>
            )}
          </div>

          {loopback ? (
            <div className="rounded-lg border border-[#fedf89] bg-[#fffaeb] px-3 py-2 text-xs leading-5 text-[#93370d]">
              {t("This server uses localhost. A phone cannot reach your computer through this address; connect the desktop to a LAN or public server URL before scanning.")}
            </div>
          ) : null}

          <p className="text-center text-xs leading-5 text-[#667085]">
            {mode === "self"
              ? t("You can still enter the server URL and account details manually in Loom Mobile.")
              : t("The recipient will choose or create their own identity in Loom Mobile.")}
          </p>
        </div>
      </div>
    </div>,
    document.body,
  );
}

function MobileConnectModeButton({
  active,
  children,
  onClick,
}: {
  active: boolean;
  children: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "rounded-lg px-3 py-2 text-xs font-semibold transition",
        active
          ? "bg-white text-[#303849] shadow-sm"
          : "text-[#667085] hover:bg-white/70 hover:text-[#303849]",
      )}
      aria-pressed={active}
      onClick={onClick}
    >
      {children}
    </button>
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

