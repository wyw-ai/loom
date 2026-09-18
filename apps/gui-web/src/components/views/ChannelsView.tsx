import {
  Fragment,
  useState,
} from "react";
import { AvatarStack } from "@/components/agent/AvatarStack";
import { ChannelDeleteConfirm } from "@/components/channel/ChannelDeleteConfirm";
import { EmptyState } from "@/components/shared/EmptyState";
import { channelGroupSections, channelTopic } from "@/lib/channel-utils";
import { ungroupedChannelGroupId } from "@/lib/constants";
import { capitalize, fallbackActor } from "@/lib/format-utils";
import { cn } from "@/lib/utils";
import { Clock, Hash, Loader2, Lock, Search, Trash2 } from "lucide-react";
import type { Actor, Channel, Thread } from "@/ipc/types";
import type { ChannelGroup } from "@/lib/types";

export function ChannelsView({
  actors,
  busy,
  channels,
  channelGroups,
  threadsByChannel,
  activeChannel,
  onSelectChannel,
  onDeleteChannel,
}: {
  actors: Record<string, Actor>;
  busy: string | null;
  channels: Channel[];
  channelGroups: ChannelGroup[];
  threadsByChannel: Record<string, Thread[]>;
  activeChannel: Channel | null;
  onSelectChannel: (channelId: string) => void;
  onDeleteChannel: (channel: Channel) => void;
}) {
  const [query, setQuery] = useState("");
  const [deleteChannelId, setDeleteChannelId] = useState<string | null>(null);
  const filteredChannels = channels.filter((channel) => {
    const text = `${channel.title} ${channel.topic ?? ""} ${channel.visibility}`.toLowerCase();
    return text.includes(query.trim().toLowerCase());
  });
  const sections = channelGroupSections(channelGroups, filteredChannels);
  const hasUserChannelGroups = channelGroups.some(
    (group) => group.id !== ungroupedChannelGroupId,
  );
  const closeDeleteConfirm = () => {
    setDeleteChannelId(null);
  };
  return (
    <section className="flex min-h-0 flex-1 flex-col bg-white">
      <div className="flex h-[96px] shrink-0 items-center justify-between border-b border-[#e2e6ef] bg-white px-6">
        <div>
          <h1 className="text-[22px] font-bold text-[#111827]">Channels</h1>
          <p className="mt-1 text-sm text-[#485063]">
            Durable spaces for teams and topics. Create channels or sections from the sidebar plus.
          </p>
        </div>
        <div className="text-sm font-semibold text-[#667085]">
          {filteredChannels.length} visible
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto bg-[#fbfbfd] p-6 soft-scrollbar">
        <label className="mb-5 flex h-10 max-w-[340px] items-center gap-2 rounded-lg border border-[#dfe3ec] bg-white px-3 text-[#667085]">
          <Search size={16} />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search channels"
            className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-[#8a93a5]"
          />
        </label>
        <div className="channel-table">
          <div className="channel-table-head">
            <span>Channel</span>
            <span>Type</span>
            <span>Members</span>
            <span>Activity</span>
            <span className="text-right">Actions</span>
          </div>
          {sections.map((section) => (
            <div key={section.id}>
              {(section.local || hasUserChannelGroups) && (
                <div className="channel-table-group">
                  {section.title}
                  <span>({section.channels.length} channels)</span>
                </div>
              )}
              {section.channels.map((channel) => {
                const threads = threadsByChannel[channel.id] ?? [];
                const deleteBusy = busy === `channel:delete:${channel.id}`;
                return (
                  <Fragment key={channel.id}>
                    <ChannelTableRow
                      actors={actors}
                      channel={channel}
                      deleteBusy={deleteBusy}
                      selected={activeChannel?.id === channel.id}
                      threads={threads}
                      onRequestDelete={() => {
                        setDeleteChannelId(channel.id);
                      }}
                      onSelect={() => onSelectChannel(channel.id)}
                    />
                    {deleteChannelId === channel.id && (
                      <ChannelDeleteConfirm
                        channel={channel}
                        deleteBusy={deleteBusy}
                        onCancel={closeDeleteConfirm}
                        onConfirm={() => {
                          onDeleteChannel(channel);
                          closeDeleteConfirm();
                        }}
                      />
                    )}
                  </Fragment>
                );
              })}
            </div>
          ))}
          {filteredChannels.length === 0 && <EmptyState icon={Hash} text="No channels." />}
        </div>
      </div>
    </section>
  );
}


export function ChannelTableRow({
  actors,
  channel,
  deleteBusy,
  selected,
  threads,
  onRequestDelete,
  onSelect,
}: {
  actors: Record<string, Actor>;
  channel: Channel;
  deleteBusy: boolean;
  selected: boolean;
  threads: Thread[];
  onRequestDelete: () => void;
  onSelect: () => void;
}) {
  const members = channel.members.map((actorId) => actors[actorId] ?? fallbackActor(actorId));
  return (
    <div
      role="button"
      tabIndex={0}
      className={cn("channel-table-row", selected && "channel-table-row-active")}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
    >
      <span className="flex min-w-0 items-center gap-3">
        <span className="channel-icon">
          <Hash size={17} />
        </span>
        <span className="min-w-0 text-left">
          <span className="block truncate text-sm font-bold text-[#111827]">
            # {channel.title}
          </span>
          <span className="block truncate text-xs text-[#596174]">
            {channelTopic(channel) || `${threads.length} active threads`}
          </span>
        </span>
      </span>
      <span className="flex items-center gap-1 text-xs font-medium text-[#667085]">
        {channel.visibility === "private" && <Lock size={12} />}
        {capitalize(channel.visibility)}
      </span>
      <span className="flex items-center">
        <AvatarStack actors={members} max={4} small />
        <span className="ml-2 text-xs font-semibold text-[#596174]">{members.length}</span>
      </span>
      <span className="flex items-center gap-2 text-xs font-medium text-[#667085]">
        <Clock size={13} />
        {threads.length > 0 ? `${threads.length} threads` : "No threads"}
      </span>
      <span className="flex justify-end">
        <button
          type="button"
          title={`Delete #${channel.title}`}
          disabled={deleteBusy}
          className="composer-icon h-8 min-w-8 text-red-500 hover:text-red-600"
          onKeyDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            onRequestDelete();
          }}
        >
          {deleteBusy ? <Loader2 className="animate-spin" size={14} /> : <Trash2 size={14} />}
        </button>
      </span>
    </div>
  );
}


