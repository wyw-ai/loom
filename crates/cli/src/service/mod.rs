//! Service-host runtime (S1 of `docs/service-plugin-system-design.md`).
//!
//! Lives inside `crates/cli` rather than as its own crate so the WS
//! [`Client`] in `crate::client` can be reused without duplication. The
//! external API (re-exports below) is structured so a future split into
//! `crates/joi-client` + `crates/service-runtime` is mostly a `mv` away.
//!
//! Boundary recap (from §6 / §8.7):
//!
//! * [`runtime::ServiceRuntime`] is the substrate plugins call into. It
//!   wraps a single WS connection bound to one service actor and exposes
//!   the §6.3 method set: actor upsert, channel-member ensure, content
//!   append, handoff, await-responds-to, dedupe, cursor, state-dir.
//! * [`plugin::ServicePlugin`] is the trait every plugin implements. The
//!   runtime owns the connection; the plugin only gets a handle and a
//!   shutdown signal via [`plugin::ServiceContext`].
//! * [`host::ServiceHost`] supervises a fleet of plugin instances loaded
//!   from `ServiceSpec` files. S1 ships with no built-in plugins — the
//!   first one (`am`) lands in S2.
//!
//! `state` is the file-ops layer for per-service private storage
//! (`~/.local/share/joi/service-host/services/<sid>/`). Pure I/O, no WS.

pub mod host;
pub mod plugin;
pub mod runtime;
pub mod state;

pub use host::ServiceHost;
// Re-exports for the eventual external surface; S1 wires only ServiceHost
// (used by `cmd::service::serve`) and `state` (used by both serve + the
// runtime). The rest are intentionally re-exported now so S2 plugin code
// can import them via `crate::service::*` without churning module paths.
#[allow(unused_imports)]
pub use plugin::{ServiceContext, ServicePlugin, ShutdownSignal};
#[allow(unused_imports)]
pub use runtime::ServiceRuntime;
