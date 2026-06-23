import type { ActionChoice, BodyPoll, ThreadActivityStats } from "@/lib/types";
import type { Actor, Message, Thread } from "@/ipc/types";
import { formatTime, shortId } from "@/lib/utils";
import {
  actorName,
  fallbackActor,
  parseMessageDate,
  startOfLocalDay,
  uniqueStrings,
  upsert,
} from "@/lib/format-utils";

// ---------------------------------------------------------------------------
// Message metadata helpers
// ---------------------------------------------------------------------------

export function metadataString(meta: unknown, keys: string[]) {
  if (!meta || typeof meta !== "object") return null;
  const record = meta as Record<string, unknown>;
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) return value;
  }
  return null;
}

export function metadataNumber(meta: unknown, keys: string[]) {
  if (!meta || typeof meta !== "object") return null;
  const record = meta as Record<string, unknown>;
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  return null;
}

export function metadataStringArray(meta: unknown, keys: string[]) {
  if (!meta || typeof meta !== "object") return [];
  const record = meta as Record<string, unknown>;
  for (const key of keys) {
    const value = record[key];
    if (Array.isArray(value)) {
      return value.filter(
        (item): item is string => typeof item === "string" && item.length > 0,
      );
    }
  }
  return [];
}

export function numberMetadata(message: Message, key: string) {
  const value = message.metadata?.[key];
  return typeof value === "number" ? value : null;
}

// ---------------------------------------------------------------------------
// Message normalization & upsert
// ---------------------------------------------------------------------------

export function normalizeMessage(message: Message): Message {
  return {
    ...message,
    attachments: message.attachments ?? [],
    reactions: message.reactions ?? [],
  };
}

export function upsertMessage(items: Message[], message: Message) {
  return upsert(items, normalizeMessage(message));
}

// ---------------------------------------------------------------------------
// Message sorting & grouping
// ---------------------------------------------------------------------------

export function sortMessages(items: Message[]) {
  return [...items].sort((a, b) => a.createdAt.localeCompare(b.createdAt));
}

export function groupMessagesByDate(messages: Message[]) {
  const groups = new Map<
    string,
    { key: string; label: string; messages: Message[] }
  >();
  for (const message of messages) {
    const key = messageDateKey(message.createdAt);
    const group = groups.get(key);
    if (group) {
      group.messages.push(message);
    } else {
      groups.set(key, {
        key,
        label: messageDateLabel(message.createdAt),
        messages: [message],
      });
    }
  }
  return Array.from(groups.values());
}

export function messageDateKey(value: string) {
  const date = parseMessageDate(value);
  if (!date) return "undated";
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function messageDateLabel(value: string) {
  const date = parseMessageDate(value);
  if (!date) return "Undated";
  const today = startOfLocalDay(new Date());
  const day = startOfLocalDay(date);
  const diffDays = Math.round((today.getTime() - day.getTime()) / 86_400_000);
  if (diffDays === 0) return "Today";
  if (diffDays === 1) return "Yesterday";
  const sameYear = date.getFullYear() === today.getFullYear();
  return new Intl.DateTimeFormat(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  }).format(date);
}

// ---------------------------------------------------------------------------
// Message kind / intent helpers
// ---------------------------------------------------------------------------

export function messageKind(message: Message) {
  const kind = message.metadata?.kind;
  return typeof kind === "string" ? kind : "chat";
}

export function messageIsActionRequestFor(message: Message, actorId: string | null) {
  if (!actorId || messageKind(message) !== "action.request") return false;
  return message.audience.some(
    (audience) => audience.kind === "actor" && audience.id === actorId,
  );
}

// ---------------------------------------------------------------------------
// Action choices & polls
// ---------------------------------------------------------------------------

export function votesFromChoice(record: Record<string, unknown>) {
  for (const key of ["votes", "voteCount", "count"]) {
    const value = record[key];
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  const actorIds = record.actorIds ?? record.voterIds ?? record.votesByActor;
  if (Array.isArray(actorIds)) return actorIds.length;
  return undefined;
}

export function actionChoices(message: Message): ActionChoice[] {
  const raw = message.metadata?.choices;
  if (Array.isArray(raw)) {
    const parsed = raw
      .flatMap((choice): ActionChoice[] => {
        if (!choice || typeof choice !== "object") return [];
        const record = choice as Record<string, unknown>;
        const id = String(record.id ?? record.label ?? "");
        if (!id) return [];
        const label = String(record.label ?? id);
        return [
          {
            id,
            label,
            accepted: !/reject|decline|cancel|no/i.test(label),
            votes: votesFromChoice(record),
          },
        ];
      });
    if (parsed.length > 0) return parsed;
  }
  return [];
}

export function bodyPollFromMessage(message: Message): BodyPoll | null {
  const lines = message.body.split("\n");
  const choices: ActionChoice[] = [];
  const questionLines: string[] = [];
  let foundChoice = false;
  for (const line of lines) {
    const match = /^\s*([A-Za-z])[\).]\s+(.+?)\s*$/.exec(line);
    if (match) {
      foundChoice = true;
      choices.push({
        id: match[1].toUpperCase(),
        label: match[2],
        accepted: true,
      });
    } else if (!foundChoice || line.trim()) {
      questionLines.push(line);
    }
  }
  if (choices.length < 2) return null;
  const question = questionLines.join("\n").trim() || messageTitle(message);
  return { question, choices };
}

// ---------------------------------------------------------------------------
// Message title & attachment helpers
// ---------------------------------------------------------------------------

export function messageTitle(message: Message) {
  const title = message.metadata?.title;
  if (typeof title === "string" && title.trim()) return title;
  return message.body.trim().split("\n")[0] || shortId(message.id);
}

export function metadataText(message: Message) {
  const reason = message.metadata?.reason;
  if (typeof reason === "string") return reason;
  return messageTitle(message);
}

export function threadTitle(message: Message) {
  return messageTitle(message).slice(0, 80);
}

export function attachmentTitle(value: string) {
  const clean = value.trim();
  if (!clean) return "Attachment";
  try {
    const url = new URL(clean);
    return decodeURIComponent(url.pathname.split("/").filter(Boolean).at(-1) ?? url.hostname);
  } catch {
    return clean.split(/[\\/]/).filter(Boolean).at(-1) ?? clean;
  }
}

export function attachmentKind(value: string) {
  const lower = value.toLowerCase();
  if (lower.endsWith(".fig") || lower.includes("figma")) return "Figma File";
  if (lower.endsWith(".pdf")) return "PDF File";
  if (lower.endsWith(".doc") || lower.endsWith(".docx") || lower.includes("doc")) {
    return "Google Doc";
  }
  if (lower.endsWith(".sheet") || lower.endsWith(".xlsx") || lower.endsWith(".csv")) {
    return "Spreadsheet";
  }
  if (lower.match(/\.(png|jpe?g|webp|gif)$/)) return "Image";
  return "File";
}

// ---------------------------------------------------------------------------
// Thread root eligibility
// ---------------------------------------------------------------------------

export function canUseAsThreadRoot(message: Message) {
  return (
    message.kind !== "system" &&
    message.scope.kind === "channel" &&
    !message.parentMessageId &&
    !message.threadRootMessageId
  );
}

// ---------------------------------------------------------------------------
// Workflow message helpers
// ---------------------------------------------------------------------------

export function isWorkflowMessage(message: Message) {
  return (
    message.kind === "task_update" ||
    message.intent === "assign_task" ||
    typeof message.metadata?.assignmentId === "string" ||
    /^Assignment\s+\S+.*\bcompleted\b/i.test(message.body.trim())
  );
}

export function isWorkflowResultMessage(message: Message, workflowSourceIds: Set<string>) {
  return (
    message.kind === "agent" &&
    Boolean(message.parentMessageId && workflowSourceIds.has(message.parentMessageId))
  );
}

export function isHiddenProtocolMessage(message: Message) {
  return /^(accepted|declined):\s*(accepted|declined)$/i.test(message.body.trim());
}

export function workflowSummary(message: Message, actors: Record<string, Actor>) {
  const taskNumber = numberMetadata(message, "taskNumber");
  const taskLabel = taskNumber ? `Task #${taskNumber}` : "Task";
  const recipient = message.audience.find((audience) => audience.kind === "actor")?.id;
  if (message.intent === "assign_task") {
    return recipient
      ? `${taskLabel} assigned to ${actorName(actors, recipient)}`
      : `${taskLabel} assigned`;
  }
  if (/completed/i.test(message.body)) {
    return `${taskLabel} completed`;
  }
  return `${taskLabel} updated`;
}

export function workflowResultSummary(message: Message) {
  const resultLine = message.body
    .split("\n")
    .map((line) => line.trim())
    .find((line) => /^Result summary:/i.test(line));
  if (resultLine) return resultLine.replace(/^Result summary:\s*/i, "");

  const usefulLines = message.body
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .filter((line) => !/^let me\b/i.test(line))
    .filter((line) => !/\bloom\b.*\b(cli|socket|PATH)\b/i.test(line))
    .filter((line) => !/^found a loom binary/i.test(line));
  return usefulLines.at(-1) ?? "Task result posted.";
}

// ---------------------------------------------------------------------------
// Thread activity stats
// ---------------------------------------------------------------------------

export function threadParticipants(
  thread: Thread,
  actors: Record<string, Actor>,
  rootAuthor?: Actor,
  stats?: ThreadActivityStats,
) {
  const metaActorIds = stats?.participantActorIds.length
    ? stats.participantActorIds
    : metadataStringArray(thread._meta, [
        "participantActorIds",
        "participants",
        "replyActorIds",
      ]);
  const candidates = [
    ...(rootAuthor ? [rootAuthor] : []),
    ...metaActorIds.map((actorId) => actors[actorId] ?? fallbackActor(actorId)),
  ];
  const seen = new Set<string>();
  return candidates.filter((actor) => {
    if (seen.has(actor.id)) return false;
    seen.add(actor.id);
    return true;
  });
}

export function threadReplyCount(thread: Thread, stats?: ThreadActivityStats) {
  if (typeof stats?.replyCount === "number") return stats.replyCount;
  const replyCount = metadataNumber(thread._meta, ["replyCount", "replies"]);
  if (replyCount !== null) return Math.max(0, replyCount);
  const messageCount = metadataNumber(thread._meta, ["messageCount"]);
  return messageCount === null ? null : Math.max(0, messageCount - 1);
}

export function threadLastReplyLabel(thread: Thread, stats?: ThreadActivityStats) {
  const raw =
    stats?.lastReplyAt ??
    metadataString(thread._meta, ["lastReplyAt", "lastMessageAt", "updatedAt"]);
  return raw ? formatTime(raw) : null;
}

export function emptyThreadStats(): ThreadActivityStats {
  return {
    replyCount: 0,
    replyMessageIds: [],
    participantActorIds: [],
    hasMoreReplies: false,
    lastReplyAt: null,
  };
}

export function threadStatsFromMessages(
  messages: Message[],
  hasMoreReplies = false,
): ThreadActivityStats {
  const sorted = sortMessages(messages).filter(
    (message) => !isHiddenProtocolMessage(message),
  );
  const participantActorIds = uniqueStrings(
    sorted.map((message) => message.authorActorId),
  );
  return {
    replyCount: sorted.length,
    replyMessageIds: sorted.map((message) => message.id),
    participantActorIds,
    hasMoreReplies,
    lastReplyAt: sorted.at(-1)?.createdAt ?? null,
  };
}

export function upsertThreadStatsMessage(
  current: Record<string, ThreadActivityStats>,
  message: Message,
) {
  if (message.scope.kind !== "thread" || isHiddenProtocolMessage(message)) return current;
  const previous = current[message.scope.id] ?? emptyThreadStats();
  const knownMessage = previous.replyMessageIds.includes(message.id);
  return {
    ...current,
    [message.scope.id]: {
      replyCount: knownMessage ? previous.replyCount : previous.replyCount + 1,
      replyMessageIds: knownMessage
        ? previous.replyMessageIds
        : [...previous.replyMessageIds, message.id],
      participantActorIds: uniqueStrings([
        ...previous.participantActorIds,
        message.authorActorId,
      ]),
      hasMoreReplies: previous.hasMoreReplies,
      lastReplyAt:
        !previous.lastReplyAt || message.createdAt > previous.lastReplyAt
          ? message.createdAt
          : previous.lastReplyAt,
    },
  };
}

// ---------------------------------------------------------------------------
// Channel / thread routing helpers
// ---------------------------------------------------------------------------

export function channelFromMessage(message: Message) {
  if (message.scope.kind === "channel") return message.scope.id;
  const match = message.target.match(/^#([^:]+)/);
  return match?.[1] ?? message.scope.id;
}

export function threadIdForMessage(
  threadsByChannel: Record<string, Thread[]>,
  message: Message,
) {
  const channelId = channelFromMessage(message);
  const root = message.threadRootMessageId ?? message.parentMessageId ?? message.id;
  return (
    threadsByChannel[channelId]?.find((thread) => thread.rootMessageId === root)
      ?.id ?? null
  );
}

export type MarkdownNode = {
  type?: string;
  value?: string;
  children?: MarkdownNode[];
  url?: string;
  title?: string | null;
};
