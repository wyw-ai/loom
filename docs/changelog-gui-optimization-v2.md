# Change Log — GUI Optimization Iteration v2

| Field | Value |
|-------|-------|
| **Task** | task_2c7b14e6cbc2 (#2) |
| **Branch** | `fix/gui-optimization-v2` |
| **Merge base** | `4befc65` (main) |
| **Commit range** | `ae0c48a` → `0eaaade` (8 commits) |
| **Scope** | `apps/gui-web` (React 18 + TypeScript + Vite + TailwindCSS + Tauri) |
| **QA verdict** | PASS — 34/35 ACs pass, 1 conditional cleared (orphaned `useStores.ts` removed) |
| **Date** | 2026-07-09 |

---

## Summary

This iteration delivered six GUI optimization items for the Loom desktop GUI (`apps/gui-web`): a major decomposition of the monolithic `App.tsx` (2414 → 441 lines), scroll performance improvements with jump buttons, a WeChat-style resizable message input, auto-scroll on thread entry, and full English-language conversion of all user-facing UI text. No behavior changes were introduced — all items are pure refactors or additive UX enhancements.

**New dependencies**: `re-resizable ^6.11.2` (5.0 KB gz), `react-textarea-autosize ^8.5.9` (1.6 KB gz). Total bundle impact: ~6.6 KB gz.

**Diff stats**: 30 files changed, +4,291 / −2,473 lines.

---

## Item 1 — App.tsx Decomposition

### What changed

`App.tsx` was a 2,414-line "god component" mixing store wiring, derived state, stream handling, connection lifecycle, scope subscriptions, ~30 async action handlers, channel-group CRUD, panel-resize logic, and a 350-line JSX render tree. It was decomposed into 14 new files across `hooks/`, `containers/`, and `lib/`, leaving `App.tsx` as a 441-line thin orchestrator.

### Extracted modules

#### `lib/derived.ts`
Pure selector functions that compute derived state from raw store data: active channel, active thread, target scope, visible channels, thread list, actor list, machine list. Replaces ~60 lines of inline derived-value calculations previously scattered in `App.tsx`.

#### Hooks (`src/hooks/`)

| File | Lines | Responsibility |
|------|-------|----------------|
| `useActions.ts` | 1,017 | All async action handlers: account (login/logout/avatar), workspace (add/remove/select/connect), channel (create/rename/delete/invite/revoke), message (send/sendThread/sendDirect/startThread), agent (create/remove/update/skills), machine (create/remove/startLocalHost/check), reactions (toggle/answer). Factory hook receiving stores + refs + derived state. |
| `useAppStores.ts` | 102 | Consolidates all Zustand store selectors (connection, UI, channel, message, actor, task, usage) into a single hook. Eliminates ~100 individual `useXStore((s) => s.field)` calls from App.tsx. |
| `useAppEffects.ts` | 64 | UI-level effects: viewport-width tracking, panel-size persistence, channel-group loading from localStorage, direct-message state reset on view switch, agent-actor-scope initialization. |
| `useWorkspaceConnection.ts` | 242 | Workspace lifecycle: config loading, workspace connection, machine management, agent form normalization. Extracts `loadWorkspaceData()`, `loadMachines()`, `loadConfig()`, `connectWorkspace()`. |
| `useStreamHandler.ts` | 288 | WebSocket stream event handler: processes incoming `StreamUpdate` events, dispatches to message/channel/actor/task/usage stores, handles channel deletion, thread updates, and direct-message routing. |
| `useChannelGroups.ts` | 99 | Channel-group CRUD: add/rename/remove/toggle-visibility/move channels between groups. Persists to localStorage via `saveChannelGroups()`. |
| `useConnectionLifecycle.ts` | 158 | Connection effects: WebSocket connect/disconnect/reconnect with exponential backoff, machine-status polling interval, stream subscription setup/teardown. |
| `useChannelScope.ts` | 234 | Scope subscription effects: subscribes/unsubscribes to channel and thread scopes, loads message lists on scope change, manages thread activity stats, handles direct-message scope resolution. |
| `usePanelResize.ts` | 116 | Panel resize drag logic for sidebar and detail panels: pointer-event handlers, min-width constraints, breakpoint-aware detail panel visibility, panel-size persistence. |

#### Containers (`src/containers/`)

| File | Lines | Responsibility |
|------|-------|----------------|
| `MainContent.tsx` | 378 | View-switching render branch: renders chat / threads / channels / direct messages / inbox / tasks / spaces / account / settings views based on `view` state. Receives all props from `WorkspaceShell`. |
| `WorkspaceShell.tsx` | 374 | Top-level layout shell: composes Rail (navigation) + Sidebar (channel list) + MainContent + detail panel + notice toast. Manages panel sizing and resize state. |

### Constraints met

- **Zero circular deps**: hooks/actions import from stores and lib only; containers import from hooks/components; `App.tsx` imports from all.
- **No behavior change**: pure structural extraction; all functionality preserved.
- **tsc clean**: `noUnusedLocals` + `noUnusedParameters` enabled — every extracted symbol is consumed.

---

## Item 2 — Scroll Performance + Jump Buttons

### Virtualization

`react-virtuoso` v4.18.7 was already installed and active in `MessageFeed.tsx` and `ThreadPanel.tsx`. The `Virtuoso` component renders only visible DOM nodes via windowing, handling 1000+ messages at ≥30fps. No new dependency was needed.

### Jump buttons

New file `src/components/chat/ScrollJumpButtons.tsx` (42 lines) provides two floating overlay buttons inside the message feed container:

- **Jump-to-top** (`ChevronUp` icon): visible when `firstVisibleIndex > 5` (scrolled away from top).
- **Jump-to-bottom** (`ChevronDown` icon): visible when `lastVisibleIndex < feedItems.length - 5` (scrolled away from bottom).

Both buttons use `virtuosoRef.scrollToIndex()` with `behavior: "smooth"`.

### Design tokens

Buttons styled with consistent design tokens: `border-[#dfe3ec]`, `text-[#667085]`, `bg-white`, `shadow-md`, `hover:bg-[#f3f4f6]`, `hover:text-[#1f2937]`. Positioned `absolute bottom-4 right-4 z-10` with `pointer-events-none` on the container and `pointer-events-auto` on each button to prevent overlap with message content.

---

## Item 3 — Resizable Message Input (WeChat-style Hybrid)

### Overview

The message composer now supports a hybrid resize model inspired by WeChat: auto-grow by default, with manual drag-to-resize via a top-edge handle. The user's preferred height persists across sessions.

### New dependencies

| Library | Version | Bundle (gz) | Purpose |
|---------|---------|-------------|---------|
| `react-textarea-autosize` | ^8.5.9 | 1.6 KB | Auto-expanding textarea (grows with content up to max rows) |
| `re-resizable` | ^6.11.2 | 5.0 KB | Single-element directional resize with drag handle |

> **ARCH divergence from STRAT**: STRAT recommended `react-resizable-panels` for the drag handle. ARCH overrode this with `re-resizable` because `react-resizable-panels` is designed for panel-group layouts (splitting containers into sibling regions), not for resizing a single element's height. `re-resizable` wraps any element and provides directional handles with `enable={{ top: true }}`.

### New files

| File | Responsibility |
|------|----------------|
| `src/lib/composer-utils.ts` (67 lines) | Constants: `COMPOSER_MIN_HEIGHT` (44px), `COMPOSER_MAX_HEIGHT_RATIO` (0.5 viewport), `COMPOSER_AUTO_MAX_ROWS` (5), thread-specific constraints (42px min, 80px cap). localStorage helpers: `loadComposerHeight()`, `saveComposerHeight()`, `clearComposerHeight()` — keyed by `"loom:composer-height"` with `{ channel: number, thread: number }` schema. |
| `src/store/composerStore.ts` (72 lines) | Zustand store managing per-composer-type state: `mode` ("auto" \| "manual"), `manualHeight` (px). Actions: `setManualHeight()` (drag → lock + persist), `resetToAuto()` (double-click → unlock + clear persistence). Loads initial state from localStorage on store creation. |
| `src/components/ui/AutoGrowTextarea.tsx` (65 lines) | Thin wrapper around `react-textarea-autosize`. In auto mode: grows up to `maxRows={5}` then scrolls. In manual mode (`fixedHeight` set): renders a plain `<textarea>` with fixed height and `overflowY: auto`. |
| `src/components/chat/ComposerResizeHandle.tsx` (67 lines) | Top-edge drag handle with full keyboard accessibility: `role="separator"`, `aria-orientation="horizontal"`, `tabIndex={0}`, `aria-label` in English. `ArrowUp`/`ArrowDown` adjust height ±8px. `Enter` or double-click resets to auto mode. Visual indicator: 10×4px rounded bar, `bg-[#d0d4de]` → `bg-[#503ed4]` on hover. |

### Modified files

- `Composer.tsx` — refactored to wrap content in `<Resizable>` with `enable={{ top: true }}`, integrating `AutoGrowTextarea` + `ComposerResizeHandle`. Props interface unchanged — zero call-site changes.
- `ThreadComposer.tsx` — same treatment with smaller constraints (42px min, 300px max).

### Mode transition logic

| Trigger | From → To | Action |
|---------|-----------|--------|
| User drags handle | auto → manual | Lock height, persist to localStorage |
| Double-click handle / Enter key | manual → auto | Unlock, resume auto-grow, clear persistence |
| User drags handle (already manual) | manual → manual | Update height, persist |

### Constraints

- **Min height**: 44px (channel), 42px (thread) — Apple HIG touch target.
- **Max height**: 50% viewport (channel), 300px (thread) — message list never disappears.
- **Layout**: composer `<footer>` is `shrink-0` in a flex column; height growth shrinks `MessageFeed` (`flex-1`) above it. Virtuoso handles reduced viewport automatically.

---

## Item 4 — Auto-scroll on Thread Entry

### What changed

When switching threads or channels, the message list now smoothly scrolls to the latest message after initial load.

### Implementation (in `MessageFeed.tsx`)

```typescript
useEffect(() => {
  if (feedItems.length === 0) return;
  const threshold = 200;
  const timer = setTimeout(() => {
    virtuosoRef.current?.scrollToIndex({
      index: feedItems.length - 1,
      behavior: feedItems.length > threshold ? "auto" : "smooth",
    });
  }, 50);
  return () => clearTimeout(timer);
}, [feedKey]); // Only on scope switch
```

### Behavior

- **50ms delay**: allows Virtuoso to render items before scrolling; completes before user interaction.
- **Adaptive scroll mode**: `>200` messages → `behavior: "auto"` (instant, avoids janky long animation); `≤200` messages → `behavior: "smooth"` (pleasant UX).
- **Trigger**: `feedKey` change only (scope switch), not on every new message.

---

## Item 5 — English Text Content

### What changed

All user-facing UI text in `apps/gui-web/src/` was converted from Chinese to English. This is a one-way migration to a single English locale — no i18n framework was added (would be over-engineering for a single-locale target).

### Approach

Direct string replacement across 7 files in P1 (commit `ae0c48a`), with additional strings in P9 (commit `0eaaade`) for new composer components.

### Verification

```bash
perl -CSD -ne 'print if /[\x{4e00}-\x{9fff}]/' src/**/*.{ts,tsx,css}
```

Returns **zero results** — no Chinese characters remain in `src/`.

---

## Item 6 — Agent Status Text

### What changed

All agent status labels across the application were converted to English. This is a subset of Item 5, called out separately because of the breadth of status-related text.

### Affected label categories

| Category | Examples | Location |
|----------|----------|----------|
| Run status | Queued, Preparing, Thinking, Waiting for tool, Failed, Canceled | `AgentIdentityBadge.tsx` |
| Chat header status | "is thinking…", "is running a tool…", "is preparing context…", "is queued…", "is working…" | `ChatHeader.tsx` |
| Connection status | Connected, Connecting, Disconnected, Idle | `agent-utils.ts` |
| Active state | Active, Idle, Terminated, Processing | `agent-utils.ts` |
| Onboarding | Connected, Online, Prepared, Scanning PATH | `OnboardingView.tsx` |
| Entity labels | Agent, Service, Member, Threads, Tasks | `agent-utils.ts` |
| Channel panel | Online, Offline | `ChannelPanels.tsx` |
| Task board | To Do, In Progress, Done, Other | `channel-utils.ts` |

---

## Commit History

| Phase | Commit | Description |
|-------|--------|-------------|
| P1 | `ae0c48a` | Replace all Chinese UI text with English (7 files) |
| P2+P3 | `6aeaab4` | Add scroll jump buttons + auto-scroll on thread entry |
| P4 | `f6942b5` | Extract `lib/derived.ts` + `hooks/usePanelResize.ts` |
| P5 | `37e4bd4` | Extract `hooks/useStreamHandler.ts` + `hooks/useChannelGroups.ts` |
| P6 | `d8978c8` | Extract `hooks/useConnectionLifecycle.ts` + `hooks/useChannelScope.ts` |
| P7 | `4be5ce6` | Extract `hooks/useActions.ts` (all action functions) |
| P8 | `d7ddd4f` | Extract containers (`MainContent.tsx`, `WorkspaceShell.tsx`) + hooks (`useAppStores`, `useAppEffects`, `useWorkspaceConnection`) |
| P9 | `0eaaade` | Resizable message input (WeChat-style hybrid) |

---

## New Files Summary (18 files)

| Path | Item |
|------|------|
| `src/lib/derived.ts` | 1 |
| `src/lib/composer-utils.ts` | 3 |
| `src/hooks/useActions.ts` | 1 |
| `src/hooks/useAppStores.ts` | 1 |
| `src/hooks/useAppEffects.ts` | 1 |
| `src/hooks/useWorkspaceConnection.ts` | 1 |
| `src/hooks/useStreamHandler.ts` | 1 |
| `src/hooks/useChannelGroups.ts` | 1 |
| `src/hooks/useConnectionLifecycle.ts` | 1 |
| `src/hooks/useChannelScope.ts` | 1 |
| `src/hooks/usePanelResize.ts` | 1 |
| `src/containers/MainContent.tsx` | 1 |
| `src/containers/WorkspaceShell.tsx` | 1 |
| `src/components/chat/ScrollJumpButtons.tsx` | 2 |
| `src/components/chat/ComposerResizeHandle.tsx` | 3 |
| `src/components/ui/AutoGrowTextarea.tsx` | 3 |
| `src/store/composerStore.ts` | 3 |

---

## Known Limitations

1. **AC-4.4 (scroll position restore) deferred**: Restoring exact scroll position when re-entering a previously visited thread is not implemented. The current auto-scroll-to-latest behavior covers the primary use case. Position restore would require persisting `{ threadId: scrollOffset }` mapping and handling message-count changes between visits — deferred to a future iteration as low-priority.

2. **No i18n framework**: English text was hardcoded via direct string replacement. If multi-language support is needed in the future, a framework (e.g., react-i18next) will need to be introduced and all strings extracted — this is a non-trivial migration from the current approach.

3. **Pre-existing orphaned CSS keyframes**: 7 `@keyframes` definitions in `AgentIdentityBadge.css` have no references. These existed on `main` before this iteration and are not a regression. Cleanup is recommended but out of scope.

4. **Message cap at 150 per channel**: `MessageStore` caps messages at 150 per channel (LRU eviction). Virtuoso's virtualization handles 1000+ messages natively, but the store cap means only the most recent 150 are retained in memory. Increasing `MAX_CHANNEL_MESSAGES` or implementing pagination would be needed for full history browsing.

5. **Thread composer constraints are hardcoded**: Thread composer uses fixed constraints (42px min, 300px max) rather than viewport-relative values. This is intentional (thread panel is narrower) but means very small viewports may not behave optimally.

---

## Verification Evidence

| Check | Result |
|-------|--------|
| `tsc --noEmit` | EXIT_CODE=0, zero errors |
| `vite build` | 1933 modules, 719.70 KB JS / 76.99 KB CSS, 1.75s, EXIT_CODE=0 |
| App.tsx line count | 441 lines (target ≤500) |
| Chinese character scan | Zero results in `src/` |
| Debug remnants | Zero `console.log` / `debugger` / `TODO` / `FIXME` |
| Orphaned files | `useStores.ts` removed (was untracked, never committed) |
| `.gitignore` | Covers `node_modules/`, `dist/`, `.vite` — no dev artifacts tracked |

---

## Workflow Traceability

| Artifact | Message ID | Author |
|----------|------------|--------|
| PRD (Items 1-6, FINAL) | `msg_81d19023a189` | PM |
| ARCH design (Items 1,2,4,5,6) | `msg_7bf5100300f4` | ARCH |
| STRAT market research (Item 3) | `msg_d36b3fa2926e` | STRAT |
| PM AC finalization (Item 3) | `msg_2c7f8335c4e3` | PM |
| ARCH design (Item 3) | `msg_edd55026ab64` | ARCH |
| FE P1-P8 completion | `msg_520c468371c3` | FE |
| FE P9 completion | `msg_529873b2ce55` | FE |
| QA verification (all items) | `msg_8d2e7cc6521d` | QA |
