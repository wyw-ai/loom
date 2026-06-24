import { memo } from "react";
import remarkGfm from "remark-gfm";
import { actorName } from "@/lib/format-utils";
import type { Actor, MessageMention } from "@/ipc/types";
import type { Components } from "react-markdown";
import ReactMarkdown from "react-markdown";
import { actorMentionRemarkPlugin, actorMentionActorId } from "@/lib/format-utils";

// remark-gfm must run BEFORE actorMentionRemarkPlugin so GFM syntax (tables,
// strikethrough, autolinks) is parsed into mdast nodes first; actorMention then
// scans the resulting tree and skips link/linkReference nodes (see
// format-utils.ts guard), so @mentions inside table cells survive.
export const MessageMarkdown = memo(function MessageMarkdown({
  actors,
  body,
  mentions = [],
}: {
  actors: Record<string, Actor>;
  body: string;
  mentions?: MessageMention[];
}) {
  const components: Components = {
    a({ href, children, node: _node, ...props }) {
      const actorId = href ? actorMentionActorId(href) : null;
      if (actorId) {
        return (
          <span
            className="message-mention"
            data-actor-id={actorId}
            title={actorName(actors, actorId)}
          >
            {children}
          </span>
        );
      }
      return (
        <a href={href} rel="noreferrer" target="_blank" {...props}>
          {children}
        </a>
      );
    },
  };

  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, actorMentionRemarkPlugin(actors, mentions)]}
      components={components}
    >
      {body}
    </ReactMarkdown>
  );
});
