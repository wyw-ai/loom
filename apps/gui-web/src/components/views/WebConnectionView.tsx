import { useMemo, useState } from "react";
import type { FormEvent } from "react";
import { Loader2, Wifi } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { normalizeServerUrl, serverUrlPreviewPlaceholder } from "@/lib/server-url";
import { useI18n } from "@/lib/i18n";

const actorIdPattern = /^[A-Za-z0-9_.:-]{1,64}$/;

export function WebConnectionView({
  busy,
  error,
  onConnect,
}: {
  busy: string | null;
  error: string | null;
  onConnect: (args: {
    serverUrl: string;
    actorId: string;
    displayName: string;
  }) => Promise<void>;
}) {
  const { t } = useI18n();
  const [serverUrl, setServerUrl] = useState("127.0.0.1:7878");
  const [actorId, setActorId] = useState("actor_human_web_user");
  const [displayName, setDisplayName] = useState("Web User");
  const preview = useMemo(() => {
    try {
      return { value: normalizeServerUrl(serverUrl), valid: true };
    } catch {
      return { value: t("Invalid server URL"), valid: false };
    }
  }, [serverUrl, t]);
  const actorValid = actorIdPattern.test(actorId.trim());
  const connecting = busy === "web:connect";

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!preview.valid || !actorValid || connecting) return;
    await onConnect({
      serverUrl: preview.value,
      actorId: actorId.trim(),
      displayName: displayName.trim() || actorId.trim(),
    });
  }

  return (
    <main className="flex h-screen w-screen items-center justify-center overflow-y-auto bg-[#f5f6fa] px-4 py-8 text-[#111827]">
      <form
        className="w-full max-w-lg overflow-hidden rounded-2xl border border-[#dfe3ec] bg-white shadow-soft"
        onSubmit={submit}
      >
        <div className="border-b border-[#edf0f5] bg-[#fbfbfd] px-6 py-5">
          <div className="flex items-center gap-3">
            <span className="flex h-11 w-11 items-center justify-center rounded-xl bg-[#5843d7] text-white">
              <Wifi size={20} />
            </span>
            <div>
              <h1 className="text-xl font-bold">{t("Connect Loom Web")}</h1>
              <p className="mt-1 text-sm text-[#667085]">
                {t("Browser mode connects directly to a Loom server.")}
              </p>
            </div>
          </div>
        </div>
        <div className="space-y-4 p-6">
          {error ? (
            <div className="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm font-medium text-red-700">
              {error}
            </div>
          ) : null}
          <label className="block">
            <span className="text-xs font-bold uppercase tracking-wide text-[#596174]">
              {t("Server URL")}
            </span>
            <Input
              className="mt-2 h-11"
              value={serverUrl}
              onChange={(event) => setServerUrl(event.target.value)}
              placeholder={serverUrlPreviewPlaceholder}
              autoFocus
            />
            <code className="mt-2 block truncate rounded-md border border-[#edf0f5] bg-[#fbfbfd] px-3 py-2 text-xs text-[#667085]">
              {preview.value}
            </code>
          </label>
          <label className="block">
            <span className="text-xs font-bold uppercase tracking-wide text-[#596174]">
              {t("Actor ID")}
            </span>
            <Input
              className="mt-2 h-11 font-mono"
              value={actorId}
              onChange={(event) => setActorId(event.target.value)}
            />
            {!actorValid ? (
              <span className="mt-1 block text-xs font-medium text-red-600">
                {t("Use letters, numbers, _, -, ., or :, up to 64 characters.")}
              </span>
            ) : null}
          </label>
          <label className="block">
            <span className="text-xs font-bold uppercase tracking-wide text-[#596174]">
              {t("Nickname")}
            </span>
            <Input
              className="mt-2 h-11"
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
            />
          </label>
          <Button
            type="submit"
            className="h-11 w-full rounded-lg"
            disabled={!preview.valid || !actorValid || connecting}
          >
            {connecting ? <Loader2 className="animate-spin" size={16} /> : <Wifi size={16} />}
            {t("Save and connect")}
          </Button>
        </div>
      </form>
    </main>
  );
}
