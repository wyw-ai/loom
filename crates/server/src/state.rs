use std::sync::Arc;

use crate::artifacts::ArtifactStore;
use crate::auth::ServerAuth;
use crate::machine_commands::MachineCommandWaiters;
use crate::scope_skills::ScopeSkills;
use crate::store::Store;
use crate::subscribe::Subscriptions;

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<ServerAuth>,
    pub store: Arc<Store>,
    pub subscriptions: Arc<Subscriptions>,
    pub artifacts: Arc<ArtifactStore>,
    pub scope_skills: Arc<ScopeSkills>,
    pub machine_commands: Arc<MachineCommandWaiters>,
}
