import { useI18n, type LanguagePreference } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export function LanguageSelector({ className }: { className?: string }) {
  const { language, locale, setLanguage, t } = useI18n();
  const options: Array<{ value: LanguagePreference; label: string }> = [
    { value: "system", label: t("System default") },
    { value: "zh-CN", label: t("Simplified Chinese") },
    { value: "en", label: t("English") },
  ];

  return (
    <div className={cn("flex flex-wrap items-center gap-3", className)}>
      <label className="text-sm font-semibold text-[#303849]" htmlFor="loom-language-select">
        {t("Language")}
      </label>
      <select
        id="loom-language-select"
        className="h-9 min-w-44 rounded-lg border border-[#dfe3ec] bg-white px-3 text-sm font-medium text-[#303849] outline-none focus:border-[#7667e8]"
        value={language}
        onChange={(event) => setLanguage(event.target.value as LanguagePreference)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <span className="text-xs text-[#667085]">
        {t("Currently using {{language}}", {
          language: locale === "zh-CN" ? t("Simplified Chinese") : t("English"),
        })}
      </span>
    </div>
  );
}
