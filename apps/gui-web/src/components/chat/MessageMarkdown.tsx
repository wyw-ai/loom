import { actorName } from "@/lib/format-utils";
import type { Actor, MessageMention } from "@/ipc/types";
import type { Components } from "react-markdown";
import ReactMarkdown from "react-markdown";
import { actorMentionRemarkPlugin, actorMentionActorId } from "@/lib/format-utils";

export function MessageMarkdown({
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
      remarkPlugins={[actorMentionRemarkPlugin(actors, mentions)]}
      components={components}
    >
      {body}
    </ReactMarkdown>
  );
}
