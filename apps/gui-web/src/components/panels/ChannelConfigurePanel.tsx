import type { Channel } from "@/ipc/types";
import { InstructionsSection } from "@/components/panels/InstructionsSection";
import { SkillsSection } from "@/components/panels/SkillsSection";

export function ChannelConfigurePanel({
  channel,
}: {
  channel: Channel;
}) {
  return (
    <div className="space-y-6">
      <InstructionsSection scope="channel" scopeId={channel.id} />
      <SkillsSection
        key={`channel:${channel.id}`}
        scope="channel"
        channelId={channel.id}
      />
    </div>
  );
}
