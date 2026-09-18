import { useEffect, useRef, useState, type ComponentType } from "react";
import { createPortal } from "react-dom";
import {
  Check,
  Globe2,
  Hash,
  Loader2,
  LockKeyhole,
  Settings,
  X,
} from "lucide-react";
import type { Channel, ChannelVisibility } from "@/ipc/types";
import { ChannelConfigurePanel } from "@/components/panels/ChannelConfigurePanel";
import { Button } from "@/components/ui/button";
import { useAnimatedDismiss } from "@/hooks/usePresence";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export function ChannelSettingsDialog({
  channel,
  connectionOpen,
  currentActorId,
  onClose,
  onUpdateVisibility,
}: {
  channel: Channel;
  connectionOpen: boolean;
  currentActorId?: string | null;
  onClose: () => void;
  onUpdateVisibility?: (
    channel: Channel,
    visibility: ChannelVisibility,
  ) => Promise<boolean>;
}) {
  const { t } = useI18n();
  const [selectedVisibility, setSelectedVisibility] = useState<ChannelVisibility>(
    channel.visibility,
  );
  const [savingVisibility, setSavingVisibility] = useState(false);
  const { closing, dismiss } = useAnimatedDismiss(onClose);
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const canManageVisibility = Boolean(
    onUpdateVisibility && currentActorId && currentActorId === channel.members[0],
  );

  useEffect(() => {
    setSelectedVisibility(channel.visibility);
  }, [channel.id, channel.visibility]);

  useEffect(() => {
    const previouslyFocused = document.activeElement as HTMLElement | null;
    closeButtonRef.current?.focus();
    const handleDialogKeys = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        dismiss();
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
  }, [dismiss]);

  const saveVisibility = async () => {
    if (
      !canManageVisibility ||
      !onUpdateVisibility ||
      !connectionOpen ||
      selectedVisibility === channel.visibility
    ) {
      return;
    }
    setSavingVisibility(true);
    try {
      await onUpdateVisibility(channel, selectedVisibility);
    } finally {
      setSavingVisibility(false);
    }
  };

  return createPortal(
    <div
      className={cn(
        "fixed inset-0 z-50 flex items-center justify-center bg-[#111827]/35 px-4 py-6 backdrop-blur-sm",
        closing && "motion-dialog-closing pointer-events-none",
      )}
      role="dialog"
      aria-modal="true"
      aria-labelledby="channel-settings-title"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) dismiss();
      }}
    >
      <div
        ref={dialogRef}
        className="flex max-h-[min(86vh,860px)] w-full max-w-3xl flex-col overflow-hidden rounded-2xl border border-[#dfe3ec] bg-white shadow-[0_28px_80px_rgb(16_24_40_/_0.22)]"
      >
        <div className="flex shrink-0 items-start justify-between gap-4 border-b border-[#edf0f5] px-6 py-5">
          <div className="flex min-w-0 items-start gap-3">
            <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[#f1efff] text-[#503ed4]">
              <Settings size={19} />
            </span>
            <div className="min-w-0">
              <h2 id="channel-settings-title" className="truncate text-lg font-bold text-[#111827]">
                {t("Channel settings")}
              </h2>
              <div className="mt-1 flex items-center gap-1.5 truncate text-sm text-[#667085]">
                <Hash size={13} />
                {channel.title}
              </div>
            </div>
          </div>
          <button
            ref={closeButtonRef}
            type="button"
            className="composer-icon h-8 min-w-8"
            title={t("Close")}
            aria-label={t("Close channel settings")}
            onClick={dismiss}
          >
            <X size={16} />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5 soft-scrollbar">
          <div className="space-y-7">
            <section>
              <div className="mb-3">
                <h3 className="text-sm font-bold text-[#111827]">{t("Visibility")}</h3>
                <p className="mt-1 text-xs leading-5 text-[#667085]">
                  {t("Choose who can find and open this channel.")}
                </p>
              </div>
              <div
                role="radiogroup"
                aria-label={t("Visibility")}
                className="grid gap-3 sm:grid-cols-2"
              >
                <VisibilityOption
                  checked={selectedVisibility === "private"}
                  description={t("Only invited members can find and open this channel.")}
                  disabled={!canManageVisibility || savingVisibility}
                  icon={LockKeyhole}
                  label={t("Private")}
                  onSelect={() => setSelectedVisibility("private")}
                />
                <VisibilityOption
                  checked={selectedVisibility === "public"}
                  description={t("Everyone on this server can find and open this channel.")}
                  disabled={!canManageVisibility || savingVisibility}
                  icon={Globe2}
                  label={t("Public")}
                  onSelect={() => setSelectedVisibility("public")}
                />
              </div>

              {!canManageVisibility ? (
                <p className="mt-3 text-xs leading-5 text-[#667085]">
                  {t("Only the channel creator can change visibility.")}
                </p>
              ) : (
                <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
                  <div className="min-w-0 flex-1">
                    {selectedVisibility !== channel.visibility && (
                      <div className="rounded-lg bg-[#fff8e7] px-3 py-2 text-xs leading-5 text-[#8a5a00]">
                        {selectedVisibility === "public"
                          ? t("Existing messages and future activity will be available to everyone on this server.")
                          : t("Only invited members will keep access to this channel.")}
                      </div>
                    )}
                    {!connectionOpen && (
                      <div className="text-xs font-medium text-[#b42318]">
                        {t("Connect to change channel visibility.")}
                      </div>
                    )}
                  </div>
                  <Button
                    size="sm"
                    disabled={
                      savingVisibility ||
                      !connectionOpen ||
                      selectedVisibility === channel.visibility
                    }
                    onClick={() => void saveVisibility()}
                  >
                    {savingVisibility && <Loader2 size={13} className="animate-spin" />}
                    {savingVisibility ? t("Saving…") : t("Save visibility")}
                  </Button>
                </div>
              )}
            </section>

            <div className="border-t border-[#edf0f5]" />
            <ChannelConfigurePanel channel={channel} />
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}

function VisibilityOption({
  checked,
  description,
  disabled,
  icon: Icon,
  label,
  onSelect,
}: {
  checked: boolean;
  description: string;
  disabled: boolean;
  icon: ComponentType<{ size?: string | number; className?: string }>;
  label: string;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      disabled={disabled}
      className={cn(
        "flex w-full items-start gap-3 rounded-xl border px-3 py-3 text-left transition-colors disabled:cursor-default disabled:opacity-80",
        checked
          ? "border-[#bdb7ff] bg-[#f7f5ff]"
          : "border-[#e2e6ef] bg-white hover:border-[#cbd1dc] hover:bg-[#fafbfc]",
      )}
      onClick={onSelect}
    >
      <span
        className={cn(
          "mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg",
          checked ? "bg-[#e8e4ff] text-[#5843d7]" : "bg-[#f2f4f7] text-[#667085]",
        )}
      >
        <Icon size={16} />
      </span>
      <span className="min-w-0">
        <span className="flex items-center gap-2 text-sm font-semibold text-[#1f2937]">
          {label}
          {checked && <Check size={14} className="text-[#5843d7]" />}
        </span>
        <span className="mt-0.5 block text-xs leading-5 text-[#667085]">
          {description}
        </span>
      </span>
    </button>
  );
}
