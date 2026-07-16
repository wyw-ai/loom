import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { errorText } from "@/lib/format-utils";
import * as ipc from "@/ipc/bridge";

export function InstructionsSection({
  scope,
  scopeId,
}: {
  scope: "channel" | "thread";
  scopeId: string;
  channelId?: string;
}) {
  const [text, setText] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedFlash, setSavedFlash] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoaded(false);
    setError(null);
    const fn =
      scope === "channel"
        ? () => ipc.channelGetInstruction(scopeId)
        : () => ipc.threadGetInstruction(scopeId);
    fn()
      .then((res) => {
        if (cancelled) return;
        setText(res.instructions ?? "");
        setLoaded(true);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(errorText(err));
        setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, [scope, scopeId]);

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    try {
      if (scope === "channel") {
        await ipc.channelSetInstruction({ channelId: scopeId, instructions: text });
      } else {
        await ipc.threadSetInstruction({ threadId: scopeId, instructions: text });
      }
      setSavedFlash(true);
      window.setTimeout(() => setSavedFlash(false), 2000);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSaving(false);
    }
  };

  const handleClear = async () => {
    setSaving(true);
    setError(null);
    try {
      if (scope === "channel") {
        await ipc.channelClearInstruction(scopeId);
      } else {
        await ipc.threadClearInstruction(scopeId);
      }
      setText("");
      setSavedFlash(true);
      window.setTimeout(() => setSavedFlash(false), 2000);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section>
      <h3 className="mb-2 text-sm font-bold text-[#111827]">📝 Instructions</h3>
      {!loaded ? (
        <div className="flex items-center gap-2 py-4 text-sm text-[#667085]">
          <Loader2 size={14} className="animate-spin" />
          Loading…
        </div>
      ) : (
        <>
          <Textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={`Enter instructions for this ${scope}…`}
            rows={8}
            disabled={saving}
            className="font-mono text-xs"
          />
          {error && (
            <p className="mt-2 text-xs text-red-500">{error}</p>
          )}
          {savedFlash && !error && (
            <p className="mt-2 text-xs text-green-600">Saved.</p>
          )}
          <div className="mt-2 flex gap-2">
            <Button size="sm" onClick={handleSave} disabled={saving}>
              {saving ? <Loader2 size={14} className="animate-spin" /> : "Save"}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={handleClear}
              disabled={saving}
            >
              Clear
            </Button>
          </div>
          {text.length === 0 && !error && (
            <p className="mt-2 text-xs text-[#667085]">
              No instructions set. Agents in this {scope} will use only their
              agent-level instructions.
            </p>
          )}
        </>
      )}
    </section>
  );
}
