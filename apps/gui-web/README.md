# loom-gui-web

React + TypeScript front-end for the Loom Desktop GUI, rendered by the Tauri
shell in `crates/gui/`.

## Run

```sh
pnpm install
make gui-dev
```

`make gui-dev` runs `cargo tauri dev` from `crates/gui/`, starts the Vite
front-end on port 5173, and opens a native window. Direct
`cargo run -p loom-gui` from the repo root is also supported for IDE restart
loops. The Tauri shell does not start `loom-server` or `loom daemon`; select a
configured workspace/server from the GUI.

Production build: `make gui-release` writes bundles under
`crates/gui/target/release/bundle/`.

## Layout

- `src/App.tsx` — shadcn-style desktop shell and protocol state reducer.
- `src/components/ui/` — small local shadcn-style primitives.
- `src/design/` — Tailwind entrypoint and CSS variables.
- `src/ipc/` — typed wrapper around Tauri commands and `loom://` events.
