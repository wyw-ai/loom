import { ActorAvatar } from "@/components/agent/ActorAvatar";
import { HostDetailSection } from "@/components/shared/UIComponents";
import type { Actor } from "@/ipc/types";
import { displayName } from "@/lib/format-utils";
import { useI18n } from "@/lib/i18n";

export function HumanListItem({ human }: { human: Actor }) {
  return (
    <div className="flex w-full items-center gap-2.5 rounded-xl border border-transparent px-3 py-2.5 text-left">
      <ActorAvatar actor={human} fallback={human.id} small />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-bold text-[#111827]">
          {displayName(human)}
        </span>
        <span className="mt-0.5 block truncate font-mono text-xs text-[#667085]">
          {human.id}
        </span>
      </span>
    </div>
  );
}

export function HumanRosterOverview({ humans }: { humans: Actor[] }) {
  const { t } = useI18n();
  return (
    <div className="min-h-full bg-white">
      <section className="border-b border-[#dfe3ec] px-6 py-6 lg:px-8">
        <h2 className="text-xl font-bold text-[#111827]">{t("Humans")}</h2>
        <p className="mt-2 text-sm text-[#667085]">
          {t("{{count}} registered on this server", { count: humans.length })}
        </p>
      </section>

      <HostDetailSection title={t("Human Directory")} count={humans.length}>
        {humans.length === 0 ? (
          <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
            {t("No humans registered.")}
          </div>
        ) : (
          <div className="space-y-2">
            {humans.map((human) => (
              <div
                key={human.id}
                className="flex items-center gap-3 rounded-xl border border-[#edf0f5] bg-[#fbfbfd] px-4 py-3"
              >
                <ActorAvatar actor={human} fallback={human.id} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-bold text-[#111827]">
                    {displayName(human)}
                  </div>
                  <div className="mt-1 truncate font-mono text-xs text-[#667085]">
                    {human.id}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </HostDetailSection>
    </div>
  );
}
