import { memo } from "react";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { actorName } from "@/lib/format-utils";
import type { Actor, MessageMention } from "@/ipc/types";
import type { Components } from "react-markdown";
import ReactMarkdown from "react-markdown";
import { actorMentionRemarkPlugin, actorMentionActorId } from "@/lib/format-utils";
import { CodeBlockWithCopy } from "@/components/chat/CodeBlockWithCopy";

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
    code({ className, children, ...props }) {
      const match = /language-(\w+)/.exec(className || "");
      const isInline = !className && !match;
      if (isInline) {
        return (
          <code className={className} {...props}>
            {children}
          </code>
        );
      }
      return (
        <CodeBlockWithCopy language={match?.[1] ?? ""} codeClassName={className}>
          {children}
        </CodeBlockWithCopy>
      );
    },
    table({ children }) {
      return (
        <div className="table-wrapper">
          <table>{children}</table>
        </div>
      );
    },
    img({ src, alt, ...props }) {
      return <img src={src} alt={alt} loading="lazy" {...props} />;
    },
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
      rehypePlugins={[[rehypeHighlight, { detect: true, ignoreMissing: true }]]}
      components={components}
    >
      {body}
    </ReactMarkdown>
  );
});
