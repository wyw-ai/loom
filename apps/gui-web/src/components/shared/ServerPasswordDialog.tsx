import { useEffect, useState } from "react";
import { Eye, EyeOff, KeyRound } from "lucide-react";
import { useI18n } from "@/lib/i18n";

export interface ServerPasswordPrompt {
  serverName: string;
  serverUrl: string;
  invalid: boolean;
}

export function ServerPasswordDialog({
  prompt,
  onCancel,
  onSubmit,
}: {
  prompt: ServerPasswordPrompt | null;
  onCancel: () => void;
  onSubmit: (password: string) => void;
}) {
  const { t } = useI18n();
  const [password, setPassword] = useState("");
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (!prompt) return;
    setPassword("");
    setVisible(false);
  }, [prompt]);

  if (!prompt) return null;

  return (
    <div
      aria-label={t("Server password required")}
      aria-modal="true"
      className="fixed inset-0 z-[100] flex items-center justify-center bg-[#111827]/45 px-4 backdrop-blur-sm"
      role="dialog"
    >
      <form
        className="w-full max-w-sm rounded-2xl border border-[#dfe3ec] bg-white p-5 shadow-2xl"
        onSubmit={(event) => {
          event.preventDefault();
          if (password) onSubmit(password);
        }}
      >
        <div className="flex items-start gap-3">
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[#eeebff] text-[#5843d7]">
            <KeyRound size={19} />
          </div>
          <div className="min-w-0">
            <h2 className="text-base font-bold text-[#111827]">{t("Enter server password")}</h2>
            <p className="mt-1 text-sm leading-5 text-[#667085]">
              {t("{{server}} requires a password before Loom can connect.", {
                server: prompt.serverName || t("this server"),
              })}
            </p>
            <p className="mt-1 truncate font-mono text-[11px] text-[#98a2b3]">{prompt.serverUrl}</p>
          </div>
        </div>

        <label className="mt-5 block text-xs font-semibold text-[#344054]" htmlFor="loom-server-password">
          {t("Password")}
        </label>
        <div className="mt-1.5 flex items-center rounded-xl border border-[#d0d5dd] bg-white focus-within:border-[#7c6ee6] focus-within:ring-2 focus-within:ring-[#7c6ee6]/15">
          <input
            autoComplete="current-password"
            autoFocus
            className="min-w-0 flex-1 bg-transparent px-3 py-2.5 text-sm text-[#111827] outline-none"
            id="loom-server-password"
            onChange={(event) => setPassword(event.target.value)}
            placeholder={t("Server password")}
            type={visible ? "text" : "password"}
            value={password}
          />
          <button
            aria-label={visible ? t("Hide password") : t("Show password")}
            className="mr-1 flex h-9 w-9 items-center justify-center rounded-lg text-[#667085] hover:bg-[#f2f4f7]"
            onClick={() => setVisible((current) => !current)}
            type="button"
          >
            {visible ? <EyeOff size={16} /> : <Eye size={16} />}
          </button>
        </div>
        {prompt.invalid && (
          <p className="mt-2 text-xs font-medium text-red-600">{t("Incorrect password. Try again.")}</p>
        )}
        <p className="mt-3 text-xs leading-5 text-[#667085]">
          {t("The password is kept only until Loom Desktop closes.")}
        </p>

        <div className="mt-5 flex justify-end gap-2">
          <button
            className="rounded-lg border border-[#d0d5dd] px-3.5 py-2 text-sm font-semibold text-[#344054] hover:bg-[#f8fafc]"
            onClick={onCancel}
            type="button"
          >
            {t("Cancel")}
          </button>
          <button
            className="rounded-lg bg-[#5843d7] px-3.5 py-2 text-sm font-semibold text-white hover:bg-[#4934c7] disabled:cursor-not-allowed disabled:opacity-50"
            disabled={!password}
            type="submit"
          >
            {t("Connect")}
          </button>
        </div>
      </form>
    </div>
  );
}
