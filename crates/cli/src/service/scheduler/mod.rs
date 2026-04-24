// Public S3 surface; first user is `joi service serve` registering the
// scheduler plugin. Silence unused warnings until that wiring lands.
#![allow(dead_code)]

//! `scheduler` plugin (S3 of `docs/service-plugin-system-design.md`).
//!
//! Runs as a long-lived [`crate::service::ServicePlugin`] inside
//! `joi service serve`: one ServiceRuntime per spec, one tokio task per
//! [`spec::JobSpec`], each task driven by a 5-field UTC cron tick. On
//! each tick the plugin runs the source (command/http), applies cursor
//! diff, dedupes, and writes one `content.add` event into the configured
//! scope — optionally with a `hands_off_to` relation if the job has a
//! `targetAgent`.
//!
//! Unlike `am` (S2) which is a one-shot CLI handler, scheduler is the
//! first plugin to actually live inside the host process and exercise
//! the §6.2 trait + §6.3 substrate end to end.

pub mod cron;
pub mod plugin;
pub mod source;
pub mod spec;

pub use plugin::SchedulerPlugin;
