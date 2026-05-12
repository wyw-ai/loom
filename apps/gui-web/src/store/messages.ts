import { create } from "zustand";

import type {
  ActionStatus,
  Bubble,
  JoiEvent,
  ScopeRef,
  TurnStreamDelta,
} from "@/ipc/types";
import { scopeKey } from "@/ipc/types";
import { summarizeActionRequest } from "@/features/chat/actionRequestSummary";

interface Announcement {
  text: string;
  actorId: string;
  ts: string;
}

interface ScopeState {
  bubbles: Bubble[];
  announcement?: Announcement;
  pendingActionIds: Set<string>;
  openTurns: Record<string, { actorId: string; openedAt: string }>;
}

interface MessagesState {
  byScope: Record<string, ScopeState>;

  ensureScope: (scope: ScopeRef) => void;
  ingestBackfill: (scope: ScopeRef, events: JoiEvent[]) => void;
  ingestEvent: (scope: ScopeRef, event: JoiEvent) => void;
  mergeDelta: (delta: TurnStreamDelta) => void;
  openTurn: (scope: ScopeRef, turnId: string, actorId: string, ts: string) => void;
  closeTurn: (scope: ScopeRef, turnId: string) => void;
  pushSystem: (scope: ScopeRef, text: string) => void;
}

function emptyScope(): ScopeState {
  return {
    bubbles: [],
    pendingActionIds: new Set(),
    openTurns: {},
  };
}

function handsOffTarget(ev: JoiEvent): string | undefined {
  const r = ev.relations.find(
    (r) => r.kind === "hands_off_to" && r.target.kind === "actor",
  );
  return r?.target.id;
}

function replyTarget(ev: JoiEvent): string | undefined {
  const r = ev.relations.find(
    (r) => r.kind === "replies_to" && r.target.kind === "event",
  );
  return r?.target.id;
}

function respondsToTarget(ev: JoiEvent): string | undefined {
  const r = ev.relations.find(
    (r) => r.kind === "responds_to" && r.target.kind === "event",
  );
  return r?.target.id;
}

function attachedArtifactIds(ev: JoiEvent): string[] {
  return ev.relations
    .filter((r) => r.kind === "attaches_artifact" && r.target.kind === "artifact")
    .map((r) => r.target.id);
}

function asString(v: unknown): string {
  return typeof v === "string" ? v : "";
}

function actionResponseStatus(
  payload: Record<string, unknown>,
  bubble: Bubble,
): ActionStatus {
  if (isQuestionRequest(bubble.requestType)) return "answered";
  const kind = asString(payload.kind);
  if (kind === "declined") return "declined";
  if (kind === "accepted") return "accepted";

  const optionId = asString(payload.optionId);
  const label =
    bubble.choices?.find((choice) => choice.id === optionId)?.label ?? optionId;
  return /reject|decline|cancel|abort|no/i.test(label)
    ? "declined"
    : "accepted";
}

function actionResponseLabel(
  payload: Record<string, unknown>,
  bubble: Bubble,
): string | undefined {
  const optionId = asString(payload.optionId);
  return (
    bubble.choices?.find((choice) => choice.id === optionId)?.label ||
    optionId ||
    asString(payload.text) ||
    undefined
  );
}

function isQuestionRequest(requestType?: string): boolean {
  return requestType === "question" || requestType === "human_decision";
}

function extractText(ev: JoiEvent): string {
  const p = ev.payload as { text?: unknown } | undefined;
  return asString(p?.text);
}

function eventMeta(ev: JoiEvent): Record<string, unknown> | undefined {
  if (ev._meta && typeof ev._meta === "object") return ev._meta;
  const payload = ev.payload as { _meta?: unknown } | undefined;
  return payload?._meta && typeof payload._meta === "object"
    ? (payload._meta as Record<string, unknown>)
    : undefined;
}

function pushOrMergeStream(bubbles: Bubble[], ev: JoiEvent): Bubble[] {
  const text = extractText(ev);
  const replyTo = replyTarget(ev);
  const turnId = ev.turnId ?? undefined;
  const meta = eventMeta(ev);
  const attachmentIds = attachedArtifactIds(ev);

  // Dedupe by event id first — the same `event.created` can arrive twice if
  // `scope/subscribe` is called repeatedly (switching back to a scope
  // re-subscribes) and the server doesn't coalesce duplicates.
  const dupIdx = bubbles.findIndex((b) => b.id === ev.id);
  if (dupIdx >= 0) {
    const next = bubbles.slice();
    next[dupIdx] = {
      ...next[dupIdx],
      text: text || next[dupIdx].text,
      meta: meta ?? next[dupIdx].meta,
      attachmentIds: attachmentIds.length
        ? attachmentIds
        : next[dupIdx].attachmentIds,
      streaming: false,
      delivery: "delivered",
      ts: ev.occurredAt,
      replyToEventId: replyTo ?? next[dupIdx].replyToEventId,
    };
    return next;
  }

  // Find the most recent stream bubble with matching actor + turn; if found,
  // this event is the canonical `content.add` landing after the stream
  // closed — overwrite text (deltas were approximate) and mark delivered.
  if (turnId) {
    for (let i = bubbles.length - 1; i >= 0; i--) {
      const b = bubbles[i];
      if (b.turnId === turnId && b.actorId === ev.actorId && b.kind === "stream") {
        const next = bubbles.slice();
        next[i] = {
          ...b,
          text: text || b.text,
          meta: meta ?? b.meta,
          attachmentIds: attachmentIds.length ? attachmentIds : b.attachmentIds,
          streaming: false,
          delivery: "delivered",
          ts: ev.occurredAt,
          id: ev.id,
          replyToEventId: replyTo ?? b.replyToEventId,
        };
        return next;
      }
    }
  }

  return [
    ...bubbles,
    {
      id: ev.id,
      actorId: ev.actorId,
      turnId,
      kind: "stream",
      text,
      ts: ev.occurredAt,
      replyToEventId: replyTo,
      meta,
      attachmentIds,
      streaming: false,
      delivery: "delivered",
    },
  ];
}

function appendOrReplace(bubbles: Bubble[], b: Bubble): Bubble[] {
  const idx = bubbles.findIndex((x) => x.id === b.id);
  if (idx >= 0) {
    const next = bubbles.slice();
    next[idx] = { ...next[idx], ...b };
    return next;
  }
  return [...bubbles, b];
}

function applyEvent(state: ScopeState, ev: JoiEvent): ScopeState {
  const payload = (ev.payload ?? {}) as Record<string, unknown>;
  switch (ev.type) {
    case "content.add": {
      const handoff = handsOffTarget(ev);
      const reply = replyTarget(ev);
      const attachmentIds = attachedArtifactIds(ev);
      if (handoff && !reply) {
        return {
          ...state,
          bubbles: appendOrReplace(state.bubbles, {
            id: ev.id,
            actorId: ev.actorId,
            turnId: ev.turnId ?? undefined,
            kind: "static",
            text: extractText(ev),
            ts: ev.occurredAt,
            meta: eventMeta(ev),
            attachmentIds,
            streaming: false,
            delivery: "delivered",
            handoffTarget: handoff,
          }),
        };
      }
      return { ...state, bubbles: pushOrMergeStream(state.bubbles, ev) };
    }
    case "action.request": {
      const summary = summarizeActionRequest(payload);
      const next = new Set(state.pendingActionIds);
      next.add(ev.id);
      return {
        ...state,
        pendingActionIds: next,
        bubbles: appendOrReplace(state.bubbles, {
          id: ev.id,
          actorId: ev.actorId,
          turnId: ev.turnId ?? undefined,
          kind: "actionRequest",
          text: `${summary.title}${
            summary.description ? `\n\n${summary.description}` : ""
          }`,
          ts: ev.occurredAt,
          streaming: false,
          delivery: "delivered",
          requestType: asString(payload.requestType),
          actionTitle: summary.title,
          actionReason: summary.reason,
          actionCommand: summary.command,
          actionRawInput: summary.rawInput,
          actionRequestId: summary.requestId,
          actionStatus: "pending",
          choices: summary.choices,
          attachmentIds: attachedArtifactIds(ev),
        }),
      };
    }
    case "action.response": {
      const requestId = respondsToTarget(ev);
      const next = new Set(state.pendingActionIds);
      if (requestId) next.delete(requestId);
      const bubbles = requestId
        ? state.bubbles.map((b) =>
            b.id === requestId
              ? {
                  ...b,
                  acknowledged: true,
                  actionStatus: actionResponseStatus(payload, b),
                  actionSelectedLabel: actionResponseLabel(payload, b),
                }
              : b,
          )
        : state.bubbles;
      return { ...state, pendingActionIds: next, bubbles };
    }
    case "announcement.set":
      return {
        ...state,
        announcement: {
          text: asString(payload.text) || asString(payload.body),
          actorId: ev.actorId,
          ts: ev.occurredAt,
        },
      };
    case "announcement.clear":
      return { ...state, announcement: undefined };
    default:
      return state;
  }
}

export const useMessages = create<MessagesState>((set) => ({
  byScope: {},

  ensureScope: (scope) =>
    set((s) => {
      const k = scopeKey(scope);
      if (s.byScope[k]) return s;
      return { byScope: { ...s.byScope, [k]: emptyScope() } };
    }),

  ingestBackfill: (scope, events) =>
    set((s) => {
      const k = scopeKey(scope);
      let st = s.byScope[k] ?? emptyScope();
      st = { ...st, bubbles: [], announcement: undefined };
      for (const ev of events) st = applyEvent(st, ev);
      return { byScope: { ...s.byScope, [k]: st } };
    }),

  ingestEvent: (scope, event) =>
    set((s) => {
      const k = scopeKey(scope);
      const st = s.byScope[k] ?? emptyScope();
      return { byScope: { ...s.byScope, [k]: applyEvent(st, event) } };
    }),

  mergeDelta: (delta) =>
    set((s) => {
      const k = scopeKey(delta.scope);
      const st = s.byScope[k] ?? emptyScope();
      const bubbles = st.bubbles.slice();
      // Find or create the streaming bubble for this turn.
      let idx = -1;
      for (let i = bubbles.length - 1; i >= 0; i--) {
        const b = bubbles[i];
        if (
          b.turnId === delta.turnId &&
          b.actorId === delta.actorId &&
          b.kind === "stream" &&
          b.streaming
        ) {
          idx = i;
          break;
        }
      }
      if (idx >= 0) {
        bubbles[idx] = { ...bubbles[idx], text: bubbles[idx].text + delta.deltaText };
      } else {
        bubbles.push({
          id: `stream:${delta.turnId}`,
          actorId: delta.actorId,
          turnId: delta.turnId,
          kind: "stream",
          text: delta.deltaText,
          ts: new Date().toISOString(),
          streaming: true,
          delivery: "na",
        });
      }
      return { byScope: { ...s.byScope, [k]: { ...st, bubbles } } };
    }),

  openTurn: (scope, turnId, actorId, ts) =>
    set((s) => {
      const k = scopeKey(scope);
      const st = s.byScope[k] ?? emptyScope();
      return {
        byScope: {
          ...s.byScope,
          [k]: {
            ...st,
            openTurns: { ...st.openTurns, [turnId]: { actorId, openedAt: ts } },
          },
        },
      };
    }),

  closeTurn: (scope, turnId) =>
    set((s) => {
      const k = scopeKey(scope);
      const st = s.byScope[k] ?? emptyScope();
      const { [turnId]: _, ...rest } = st.openTurns;
      const bubbles = st.bubbles.map((b) =>
        b.turnId === turnId && b.streaming ? { ...b, streaming: false } : b,
      );
      return {
        byScope: { ...s.byScope, [k]: { ...st, openTurns: rest, bubbles } },
      };
    }),

  pushSystem: (scope, text) =>
    set((s) => {
      const k = scopeKey(scope);
      const st = s.byScope[k] ?? emptyScope();
      return {
        byScope: {
          ...s.byScope,
          [k]: {
            ...st,
            bubbles: [
              ...st.bubbles,
              {
                id: `sys:${Date.now()}:${Math.random()}`,
                actorId: "system",
                kind: "system",
                text,
                ts: new Date().toISOString(),
                streaming: false,
                delivery: "na",
              },
            ],
          },
        },
      };
    }),
}));
