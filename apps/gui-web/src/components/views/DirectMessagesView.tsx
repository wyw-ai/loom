import {
  useState,
} from "react";
import { ActorAvatar } from "@/components/agent/ActorAvatar";
import { ActorActivityBanner } from "@/components/chat/ActorActivityBanner";
import { Composer } from "@/components/chat/Composer";
import { MessageFeed } from "@/components/chat/MessageFeed";
import { EmptyState } from "@/components/shared/EmptyState";
import { Button } from "@/components/ui/button";
import { displayName, shortActorAlias } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Bot, Hash, MessageCircle, Search } from "lucide-react";
import type { Actor, Channel, MachineInfo, Message, Run, ScopeRef } from "@/ipc/types";
import { useI18n } from "@/lib/i18n";

export function DirectMessagesView({
  actors,
  agents,
  busy,
  currentActorId,
  disabled,
  draft,
  linkedChannel,
  machines,
  runs,
  messages,
  directScope,
  anchorMessageId,
  selectedAgent,
  setDraft,
  onOpenLinkedChannel,
  onSelectAgent,
  onAnswerAction,
  onSend,
  onToggleReaction,
  onOpenAgentSettings,
}: {
  actors: Record<string, Actor>;
  agents: Actor[];
  busy: string | null;
  currentActorId: string | null;
  disabled: boolean;
  draft: string;
  linkedChannel: Channel | null;
  machines: MachineInfo[];
  runs: Record<string, Run>;
  messages: Message[];
  directScope: ScopeRef | null;
  anchorMessageId?: string | null;
  selectedAgent: Actor | null;
  setDraft: (value: string) => void;
  onOpenLinkedChannel: (channelId: string) => void;
  onSelectAgent: (actorId: string) => void;
  onAnswerAction: (message: Message, optionId: string, accepted: boolean) => void;
  onSend: () => void;
  onToggleReaction: (message: Message, emoji: string) => void;
  onOpenAgentSettings: (actorId: string) => void;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const filteredAgents = agents.filter((agent) => {
    const text = `${displayName(agent)} ${agent.id}`.toLowerCase();
    return text.includes(query.trim().toLowerCase());
  });
  const selectedBusy = selectedAgent
    ? busy === `direct:message:send:${selectedAgent.id}`
    : false;
  return (
    <section className="direct-messages-view grid min-h-0 flex-1 grid-cols-1 bg-white md:grid-cols-[minmax(260px,340px)_minmax(0,1fr)]">
      <aside className="min-h-0 border-r border-[#e2e6ef] bg-[#fbfbfd]">
        <div className="flex h-[96px] flex-col justify-center border-b border-[#e2e6ef] px-5">
          <h1 className="text-[22px] font-bold text-[#111827]">{t("Direct Messages")}</h1>
          <div className="mt-1 text-sm font-medium text-[#667085]">
            {t("{{count}} agents", { count: agents.length })}
          </div>
        </div>
        <div className="p-4">
          <label className="search-pill mb-4 h-10">
            <Search size={16} />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("Search agents")}
              className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-[#8a93a5]"
            />
          </label>
          <div className="space-y-1">
            {filteredAgents.map((agent) => {
              const selected = selectedAgent?.id === agent.id;
              return (
                <button
                  key={agent.id}
                  type="button"
                  className={cn(
                    "flex min-h-12 w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors",
                    selected
                      ? "bg-[#efecff] text-[#4f3fd7]"
                      : "text-[#303849] hover:bg-[#f0f1f8]",
                  )}
                  onClick={() => onSelectAgent(agent.id)}
                >
                  <ActorAvatar actor={agent} fallback={agent.id} small />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-bold">
                      {displayName(agent)}
                    </span>
                    <span className="block truncate text-xs font-medium text-[#667085]">
                      {shortActorAlias(agent.id)}
                    </span>
                  </span>
                </button>
              );
            })}
            {filteredAgents.length === 0 && (
              <EmptyState icon={Bot} text={t("No agents.")} />
            )}
          </div>
        </div>
      </aside>
      <div className="flex min-h-0 min-w-0 flex-col bg-white">
        {!selectedAgent ? (
          <div className="flex min-h-0 flex-1 items-center justify-center p-8">
            <EmptyState icon={MessageCircle} text={t("Select an agent.")} />
          </div>
        ) : (
          <>
            <header className="flex h-[86px] shrink-0 items-center gap-4 border-b border-[#e2e6ef] bg-white px-6">
              <ActorAvatar actor={selectedAgent} fallback={selectedAgent.id} />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[22px] font-bold leading-tight text-[#111827]">
                  {displayName(selectedAgent)}
                </div>
                <div className="mt-1 truncate text-sm text-[#485063]">
                  {selectedAgent.id}
                </div>
              </div>
              {linkedChannel && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => onOpenLinkedChannel(linkedChannel.id)}
                >
                  <Hash size={14} />
                  {linkedChannel.title}
                </Button>
              )}
            </header>
            <MessageFeed
              actors={actors}
              allowReply={false}
              allowThreads={false}
              feedKey={`direct:${selectedAgent.id}`}
              machines={machines}
              runs={runs}
              messages={messages}
              tasksBySourceMessageId={{}}
              channelThreads={[]}
              threadStatsById={{}}
              emptyText={t("No direct messages.")}
              onReply={() => {}}
              onStartThread={() => {}}
              onToggleReaction={onToggleReaction}
              onAnswerAction={onAnswerAction}
              onOpenAgentSettings={onOpenAgentSettings}
              currentActorId={currentActorId}
              busy={busy}
              anchorMessageId={anchorMessageId}
            />
            <ActorActivityBanner
              actors={actors}
              actorIds={[selectedAgent.id]}
              machines={machines}
              runs={runs}
              scope={directScope}
              enabled={!disabled}
            />
            <Composer
              draft={draft}
              setDraft={setDraft}
              disabled={disabled}
              replyTo={null}
              actorName=""
              onClearReply={() => {}}
              onSend={onSend}
              mentionAgents={[]}
              placeholder={t("Message {{name}}", { name: displayName(selectedAgent) })}
              disabledPlaceholder={t("Connect and select an agent")}
              busy={selectedBusy}
            />
          </>
        )}
      </div>
    </section>
  );
}
