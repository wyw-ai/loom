import { Megaphone } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { useActors } from "@/store/actors";

export function AnnouncementBanner({
  announcement,
}: {
  announcement: { text: string; actorId: string; ts: string };
}) {
  const actor = useActors((s) => s.byId[announcement.actorId]);
  const name = actor?.displayName || announcement.actorId;
  return (
    <div className="flex gap-3 border-b-2 border-black bg-brutal-yellow px-4 py-2 text-sm">
      <Megaphone size={16} className="mt-0.5 shrink-0 text-black" />
      <div className="min-w-0 flex-1">
        <div className="prose-announcement font-bold text-black">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>
            {announcement.text}
          </ReactMarkdown>
        </div>
        <div className="mt-1 font-mono text-xs text-black/55" title={announcement.actorId}>
          @{name} - {new Date(announcement.ts).toLocaleString()}
        </div>
      </div>
    </div>
  );
}
