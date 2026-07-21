import { useCallback, useEffect, useState } from "react";
import { Loader2, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { errorText } from "@/lib/format-utils";
import * as ipc from "@/ipc/bridge";
import type { SkillEntry } from "@/ipc/types";

export function SkillsSection({
  scope,
  channelId,
  threadId,
}: {
  scope: "channel" | "thread";
  channelId: string;
  threadId?: string;
}) {
  const [skills, setSkills] = useState<SkillEntry[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [newSource, setNewSource] = useState("");
  const [newId, setNewId] = useState("");
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async (signal: { cancelled: boolean }) => {
    setError(null);
    try {
      const res =
        scope === "channel"
          ? await ipc.channelSkillList(channelId)
          : await ipc.threadSkillList({ channelId, threadId: threadId! });
      if (signal.cancelled) return;
      setSkills(res.skills);
      setLoaded(true);
    } catch (err) {
      if (signal.cancelled) return;
      setError(errorText(err));
      setLoaded(true);
    }
  }, [scope, channelId, threadId]);

  useEffect(() => {
    // Reset baseline state for this scope instance so draft/loading do not
    // leak across scope changes.
    setLoaded(false);
    setNewSource("");
    setNewId("");
    setError(null);
    const signal = { cancelled: false };
    reload(signal);
    return () => {
      signal.cancelled = true;
    };
  }, [reload]);

  const handleAdd = async () => {
    if (!newSource.trim()) return;
    setAdding(true);
    setError(null);
    try {
      const res =
        scope === "channel"
          ? await ipc.channelSkillAdd({
              channelId,
              source: newSource.trim(),
              ...(newId.trim() ? { skillId: newId.trim() } : {}),
            })
          : await ipc.threadSkillAdd({
              channelId,
              threadId: threadId!,
              source: newSource.trim(),
              ...(newId.trim() ? { skillId: newId.trim() } : {}),
            });
      setSkills(res.skills);
      setNewSource("");
      setNewId("");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setAdding(false);
    }
  };

  const handleRemove = async (skillId: string) => {
    setError(null);
    try {
      const res =
        scope === "channel"
          ? await ipc.channelSkillRemove({ channelId, skillId })
          : await ipc.threadSkillRemove({ channelId, threadId: threadId!, skillId });
      setSkills(res.skills);
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <section>
      <h3 className="mb-2 text-sm font-bold text-[#111827]">🔧 Skills</h3>
      {!loaded ? (
        <div className="flex items-center gap-2 py-4 text-sm text-[#667085]">
          <Loader2 size={14} className="animate-spin" />
          Loading…
        </div>
      ) : (
        <>
          {error && <p className="mb-2 text-xs text-red-500">{error}</p>}
          {skills.length === 0 ? (
            <div className="rounded-lg border border-dashed border-[#dfe3ec] bg-[#fbfbfd] px-4 py-6 text-sm text-[#667085]">
              No skills mounted.
            </div>
          ) : (
            <div className="space-y-2">
              {skills.map((skill) => (
                <div
                  key={skill.id}
                  className="flex items-center justify-between rounded-lg border border-[#dfe3ec] bg-white px-4 py-3"
                >
                  <div className="min-w-0">
                    <div className="text-sm font-bold text-[#111827]">{skill.id}</div>
                    <div className="mt-0.5 truncate font-mono text-xs text-[#667085]">
                      {skill.source}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => handleRemove(skill.id)}
                    title="Remove skill"
                    className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-[#dfe3ec] bg-white text-[#9aa1ae] hover:border-red-300 hover:text-red-500"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              ))}
            </div>
          )}
          <div className="mt-3 space-y-2">
            <Input
              placeholder="Skill source path (e.g. C:/path/to/skill)"
              value={newSource}
              onChange={(e) => setNewSource(e.target.value)}
              disabled={adding}
            />
            <div className="flex gap-2">
              <Input
                placeholder="Skill ID (optional, auto-derived from path)"
                value={newId}
                onChange={(e) => setNewId(e.target.value)}
                className="flex-1"
                disabled={adding}
              />
              <Button
                size="sm"
                onClick={handleAdd}
                disabled={adding || !newSource.trim()}
              >
                {adding ? <Loader2 size={14} className="animate-spin" /> : "Add"}
              </Button>
            </div>
          </div>
        </>
      )}
    </section>
  );
}
