import { CacheManagementSection } from "@/components/settings/CacheManagementSection";
import { LanguageSelector } from "@/components/settings/LanguageSelector";
import { SettingsSection } from "@/components/settings/SettingsSection";
import { PageHeader } from "@/components/shared/PageComponents";
import { useI18n } from "@/lib/i18n";

export function SystemSettingsView() {
  const { t } = useI18n();

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title={t("System Settings")}
        detail={t("Language and local app storage")}
      />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 soft-scrollbar">
        <div className="mx-auto max-w-2xl space-y-5">
          <SettingsSection
            title={t("Language")}
            detail={t("Choose the language used by Loom Desktop.")}
          >
            <LanguageSelector />
          </SettingsSection>
          <CacheManagementSection />
        </div>
      </div>
    </section>
  );
}
