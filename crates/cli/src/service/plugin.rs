// Public S1 surface; first impl arrives in S2. Silence the dead-code
// noise until then.
#![allow(dead_code)]

//! [`ServicePlugin`] — the trait every service plugin implements — and
//! [`ServiceContext`] — the handle the host passes to `run`.
//!
//! Designed thin on purpose. Plugins own their own loops (cron tick for
//! scheduler, webhook listener subprocesses, etc.); the runtime exists
//! to give them protocol primitives. Anything plugin-specific (message
//! send-back, cron parsing, HTTP fetching) stays inside the plugin —
//! see §8.7's API-surface validation conclusion.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use proto::methods::ServiceSpec;
use tokio::sync::watch;

use super::instance::InstanceRequest;
use super::runtime::ServiceRuntime;

/// Cancellation signal carried by the host. `true` means "shut down
/// now"; plugins observe it via [`watch::Receiver::changed`] and exit
/// `run` cleanly. The host triggers it on SIGINT/SIGTERM and on
/// process-wide shutdown.
pub type ShutdownSignal = watch::Receiver<bool>;

/// Readiness hook supplied by the service host. Plugins call it after their
/// configuration has been parsed and their long-lived loop is ready to
/// operate. Calling it more than once is harmless.
#[derive(Clone, Default)]
pub struct ServiceReadiness {
    on_ready: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ServiceReadiness {
    pub(crate) fn new(on_ready: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            on_ready: Some(on_ready),
        }
    }

    pub fn mark_running(&self) {
        if let Some(on_ready) = self.on_ready.as_ref() {
            on_ready();
        }
    }
}

/// What the host hands to [`ServicePlugin::run`]. Cheap to clone the
/// inner pieces — `runtime` is an `Arc` and `shutdown` is a
/// `watch::Receiver`.
pub struct ServiceContext {
    /// Validated spec for this plugin instance. `spec.kind` matches
    /// [`ServicePlugin::kind`].
    pub spec: ServiceSpec,
    /// Filesystem path of the spec file this run was loaded from
    /// (`<specs_dir>/<id>.json` or `<specs_dir>/<id>/spec.json`).
    /// `None` for in-memory specs (programmatic [`ServiceHost::add_spec`]
    /// without a source path). Plugins that resolve `{spec.dir}` /
    /// `{bundle.dir}` placeholders look at the parent of this path.
    pub spec_path: Option<std::path::PathBuf>,
    /// Substrate for talking to loom-server. See [`ServiceRuntime`].
    pub runtime: Arc<ServiceRuntime>,
    /// Cancellation signal from the host. Plugins observe it to exit
    /// cleanly on shutdown.
    pub shutdown: ShutdownSignal,
    /// Per-instance request payload, present iff the host dispatched
    /// this run as a `lifecycle = thread_bound` instance (§4.7.3).
    /// `None` for the channel-level singleton path. Plugins that
    /// honor `params` / `scope.thread_id` placeholders read it here.
    pub instance: Option<InstanceRequest>,
    /// The host has already connected and upserted the actor when this hook
    /// is delivered. Plugins mark it running only after plugin-level config
    /// validation succeeds.
    pub readiness: ServiceReadiness,
}

#[async_trait]
pub trait ServicePlugin: Send + Sync {
    /// Plugin kind discriminator. Must match `ServiceSpec.kind` for
    /// every spec the host dispatches to this plugin. The host's
    /// registry is keyed on this string.
    fn kind(&self) -> &'static str;

    /// Long-running entry point. Runs until either the plugin decides
    /// to exit or `ctx.shutdown` flips to `true`. Returning `Ok(())`
    /// means clean exit; `Err` is logged by the host (S1: no restart;
    /// retry policy lands in a later phase).
    async fn run(&self, ctx: ServiceContext) -> Result<()>;
}
