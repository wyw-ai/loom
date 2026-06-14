import { PageHeader } from "@/components/shared/PageComponents";
import { SettingsSection } from "@/components/settings/SettingsSection";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { localServerCommand } from "@/lib/constants";
import { connectionLabel, workspaceInitials } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Check, Loader2, Plus, Trash2 } from "lucide-react";
import type { Workspace } from "@/ipc/types";
import type { ConnectionState } from "@/lib/types";

export function SpacesView({
  busy,
  connection,
  workspace,
  workspaceForm,
  setWorkspaceForm,
  workspaces,
  onAddWorkspace,
  onRemoveWorkspace,
  onSelectWorkspace,
}: {
  busy: string | null;
  connection: ConnectionState;
  workspace: Workspace | null;
  workspaceForm: { name: string; serverUrl: string };
  setWorkspaceForm: (form: { name: string; serverUrl: string }) => void;
  workspaces: Workspace[];
  onAddWorkspace: () => void;
  onRemoveWorkspace: (id: string) => void;
  onSelectWorkspace: (workspaceId: string) => void;
}) {
  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <PageHeader title="Spaces" detail="Choose or add a server connection" />
      <div className="min-h-0 flex-1 overflow-y-auto p-5 soft-scrollbar">
        <div className="mx-auto max-w-4xl space-y-4">
          <SettingsSection title="Add Space" detail="Save a connection target in the side rail.">
            <form
              className="grid gap-3 sm:grid-cols-[180px_1fr_auto]"
              onSubmit={(event) => {
                event.preventDefault();
                onAddWorkspace();
              }}
            >
              <Input
                value={workspaceForm.name}
                onChange={(event) =>
                  setWorkspaceForm({ ...workspaceForm, name: event.target.value })
                }
                placeholder="Name"
              />
              <Input
                value={workspaceForm.serverUrl}
                onChange={(event) =>
                  setWorkspaceForm({ ...workspaceForm, serverUrl: event.target.value })
                }
                placeholder="ws://127.0.0.1:7878/rpc"
              />
              <Button
                type="submit"
                disabled={
                  busy === "workspace:add" ||
                  !workspaceForm.name.trim() ||
                  !workspaceForm.serverUrl.trim()
                }
              >
                {busy === "workspace:add" ? (
                  <Loader2 className="animate-spin" size={15} />
                ) : (
                  <Plus size={15} />
                )}
                Add Space
              </Button>
            </form>
          </SettingsSection>

          <SettingsSection title="Saved Spaces" detail="The side rail uses this list for switching.">
            <div className="space-y-2">
              {workspaces.length === 0 ? (
                <div className="rounded-xl border border-dashed border-[#dfe3ec] bg-[#fbfbfd] p-4 text-sm text-[#667085]">
                  <div>No spaces configured.</div>
                  <code className="mt-2 block rounded-lg border border-[#dfe3ec] bg-white px-3 py-2 font-mono text-xs font-semibold text-[#303849]">
                    {localServerCommand}
                  </code>
                </div>
              ) : (
                workspaces.map((item) => {
                  const selected = item.id === workspace?.id;
                  const connecting = busy === `connect:${item.id}`;
                  return (
                    <div
                      key={item.id}
                      className="flex items-center gap-3 rounded-lg border border-[#dfe3ec] bg-[#fbfbfd] px-3 py-3"
                    >
                      <span
                        className={cn(
                          "flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border text-sm font-bold",
                          selected
                            ? "border-[#bdb7ff] bg-[#f1efff] text-[#5843d7]"
                            : "border-[#dfe3ec] bg-white text-[#303849]",
                        )}
                      >
                        {workspaceInitials(item)}
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="flex min-w-0 items-center gap-2">
                          <div className="truncate text-sm font-bold text-[#111827]">
                            {item.name}
                          </div>
                          <Badge
                            variant={
                              selected && connection === "open" ? "success" : "outline"
                            }
                          >
                            {selected ? connectionLabel(connection) : "Saved"}
                          </Badge>
                        </div>
                        <div className="mt-1 truncate font-mono text-xs text-[#667085]">
                          {item.serverUrl}
                        </div>
                      </div>
                      <Button
                        variant={selected && connection === "open" ? "outline" : "default"}
                        size="sm"
                        onClick={() => onSelectWorkspace(item.id)}
                        disabled={connecting}
                      >
                        {connecting ? (
                          <Loader2 className="animate-spin" size={14} />
                        ) : selected && connection === "open" ? (
                          <Check size={14} />
                        ) : null}
                        {selected && connection === "open" ? "Open" : "Connect"}
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        title="Remove space"
                        onClick={() => onRemoveWorkspace(item.id)}
                        disabled={busy === `workspace:remove:${item.id}`}
                      >
                        <Trash2 size={16} />
                      </Button>
                    </div>
                  );
                })
              )}
            </div>
          </SettingsSection>
        </div>
      </div>
    </section>
  );
}


