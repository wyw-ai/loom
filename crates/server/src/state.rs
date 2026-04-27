use std::sync::Arc;

use crate::artifacts::ArtifactStore;
use crate::store::Store;
use crate::subscribe::Subscriptions;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub subscriptions: Arc<Subscriptions>,
    pub artifacts: Arc<ArtifactStore>,
}
