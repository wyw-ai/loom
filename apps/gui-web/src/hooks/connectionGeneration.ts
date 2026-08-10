import type { ConnectionEvent } from "@/ipc/types";

/**
 * Mutable connection ordering state shared by the connect command and the
 * connection event listener. Keeping it outside React state lets an event that
 * races an IPC response invalidate that response synchronously.
 */
export interface ConnectionGenerationState {
  attempt: number;
  pendingAttempt: number | null;
  activeConnectionId: number | null;
  staleThroughConnectionId: number;
  closedConnectionIds: Set<number>;
  pendingEvents: Map<number, ConnectionEvent>;
}

export function createConnectionGenerationState(): ConnectionGenerationState {
  return {
    attempt: 0,
    pendingAttempt: null,
    activeConnectionId: null,
    staleThroughConnectionId: -1,
    closedConnectionIds: new Set<number>(),
    pendingEvents: new Map<number, ConnectionEvent>(),
  };
}

function validConnectionId(connectionId: number) {
  return Number.isSafeInteger(connectionId) && connectionId >= 0;
}

/** Start a user/automatic connect attempt and invalidate the previous socket. */
export function beginConnectionAttempt(state: ConnectionGenerationState) {
  state.attempt += 1;
  state.pendingAttempt = state.attempt;
  state.pendingEvents.clear();
  if (state.activeConnectionId !== null) {
    state.staleThroughConnectionId = Math.max(
      state.staleThroughConnectionId,
      state.activeConnectionId,
    );
    state.activeConnectionId = null;
  }
  for (const connectionId of state.closedConnectionIds) {
    if (connectionId <= state.staleThroughConnectionId) {
      state.closedConnectionIds.delete(connectionId);
    }
  }
  return state.attempt;
}

export function isCurrentConnectionAttempt(
  state: ConnectionGenerationState,
  attempt: number,
) {
  return state.attempt === attempt;
}

/**
 * Adopt a connect result only if both its caller attempt and backend generation
 * are still current. `closed` captures Closed-before-connect-resolve races.
 */
export function adoptConnectionResult(
  state: ConnectionGenerationState,
  attempt: number,
  connectionId: number,
): { current: boolean; closed: boolean; closedReason?: string } {
  if (
    !isCurrentConnectionAttempt(state, attempt) ||
    state.pendingAttempt !== attempt
  ) {
    return { current: false, closed: false };
  }

  const bufferedEvent = state.pendingEvents.get(connectionId);
  state.pendingAttempt = null;
  state.pendingEvents.clear();
  if (!validConnectionId(connectionId)) return { current: false, closed: false };

  if (bufferedEvent?.state === "closed") {
    state.closedConnectionIds.add(connectionId);
  }

  if (state.activeConnectionId === connectionId) {
    return {
      current: true,
      closed: state.closedConnectionIds.has(connectionId),
      ...(bufferedEvent?.state === "closed" && bufferedEvent.reason
        ? { closedReason: bufferedEvent.reason }
        : {}),
    };
  }

  if (connectionId <= state.staleThroughConnectionId) {
    // Web mode can return its already-open socket from a fresh connect call.
    // A current attempt may reclaim exactly the generation it invalidated.
    if (
      state.activeConnectionId === null &&
      connectionId === state.staleThroughConnectionId
    ) {
      state.activeConnectionId = connectionId;
      return {
        current: true,
        closed: state.closedConnectionIds.has(connectionId),
        ...(bufferedEvent?.state === "closed" && bufferedEvent.reason
          ? { closedReason: bufferedEvent.reason }
          : {}),
      };
    }
    return { current: false, closed: false };
  }

  if (
    state.activeConnectionId !== null &&
    connectionId < state.activeConnectionId
  ) {
    return { current: false, closed: false };
  }

  if (state.activeConnectionId !== null) {
    state.staleThroughConnectionId = Math.max(
      state.staleThroughConnectionId,
      state.activeConnectionId,
    );
  }
  state.activeConnectionId = connectionId;
  return {
    current: true,
    closed: state.closedConnectionIds.has(connectionId),
    ...(bufferedEvent?.state === "closed" && bufferedEvent.reason
      ? { closedReason: bufferedEvent.reason }
      : {}),
  };
}

/** Finish the current failed attempt without letting its buffered events leak. */
export function finishConnectionAttemptFailure(
  state: ConnectionGenerationState,
  attempt: number,
) {
  if (
    !isCurrentConnectionAttempt(state, attempt) ||
    state.pendingAttempt !== attempt
  ) {
    return false;
  }
  state.pendingAttempt = null;
  state.pendingEvents.clear();
  return true;
}

/**
 * Returns true only for an event belonging to the newest connection
 * generation. Once a generation closes, a late Open for it is also ignored.
 */
export function adoptConnectionEvent(
  state: ConnectionGenerationState,
  event: ConnectionEvent,
) {
  const connectionId = event.connectionId;
  if (!validConnectionId(connectionId)) return false;

  // Until the newest connect command tells us its connection id, an event may
  // belong to an older overlapping command. Cache it, but never drive the UI.
  if (state.pendingAttempt !== null) {
    const buffered = state.pendingEvents.get(connectionId);
    if (buffered?.state !== "closed") {
      state.pendingEvents.set(connectionId, event);
    }
    return false;
  }

  if (state.activeConnectionId === connectionId) {
    if (event.state === "open" && state.closedConnectionIds.has(connectionId)) {
      return false;
    }
    if (event.state === "closed") state.closedConnectionIds.add(connectionId);
    return true;
  }

  if (connectionId <= state.staleThroughConnectionId) return false;
  if (
    state.activeConnectionId !== null &&
    connectionId < state.activeConnectionId
  ) {
    return false;
  }

  if (state.activeConnectionId !== null) {
    state.staleThroughConnectionId = Math.max(
      state.staleThroughConnectionId,
      state.activeConnectionId,
    );
  }
  state.activeConnectionId = connectionId;
  if (event.state === "closed") state.closedConnectionIds.add(connectionId);
  return true;
}
