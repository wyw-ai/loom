# joi-gui-web

React + TypeScript front-end for the Joi Desktop GUI, rendered by the Tauri
shell in `crates/gui/`. See [docs/gui-desktop-design.md](../../docs/gui-desktop-design.md)
for the design rationale and state-model mapping.

## Run

```sh
# one-time
pnpm install

# day-to-day (from repo root)
make gui-dev
```

`make gui-dev` runs `cargo tauri dev` from `crates/gui/`, which spawns
`pnpm dev` here for the front-end on port 5173 and opens a native window
pointed at it. Edits hot-reload.

Production build: `make gui-release` → bundle in `crates/gui/target/release/bundle/`.

## Layout

- `src/design/` — CSS variables + Tailwind entrypoint. Tokens are the source
  of truth for the palette; Tailwind reads them via `tailwind.config.ts`.
- `src/ipc/` — typed wrapper around `@tauri-apps/api` (commands + event
  listeners). `types.ts` mirrors the relevant shapes from `crates/proto`.
- `src/store/` — Zustand stores, one per domain (session / channels /
  messages / inbox / ui). Mutations are driven by IPC events in `App.tsx`.
- `src/features/` — feature folders: sidebar, chat, prompt, members, inbox.
  Components only talk to stores + ipc; they never open a WebSocket of
  their own.
