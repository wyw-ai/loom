use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use proto::types::Thread;

use crate::store::Store;

/// Compatibility shim for the old server-side scope skill projection.
///
/// Skill mounting now happens in `loom agent serve` under each agent's own
/// workspace. The server keeps this type so existing mutation handlers can call
/// into it, but it intentionally no-ops instead of writing channel/thread skill
/// directories under server data.
pub struct ScopeSkills;

impl ScopeSkills {
    pub fn new(root: PathBuf, agents_root: PathBuf) -> io::Result<Self> {
        let _ = (root, agents_root);
        Ok(Self)
    }

    pub fn reconcile(&self, store: &Arc<Store>) -> io::Result<()> {
        let _ = store;
        Ok(())
    }

    pub fn sync_actor_channel_membership(
        &self,
        store: &Arc<Store>,
        channel_id: &str,
        actor_id: &str,
    ) -> io::Result<()> {
        let _ = (store, channel_id, actor_id);
        Ok(())
    }

    pub fn remove_actor_channel_membership(
        &self,
        store: &Arc<Store>,
        channel_id: &str,
        actor_id: &str,
    ) -> io::Result<()> {
        let _ = (store, channel_id, actor_id);
        Ok(())
    }

    pub fn sync_thread_channel_memberships(
        &self,
        store: &Arc<Store>,
        thread: &Thread,
    ) -> io::Result<()> {
        let _ = (store, thread);
        Ok(())
    }

    pub fn remove_thread_scope(&self, thread_id: &str) -> io::Result<()> {
        let _ = thread_id;
        Ok(())
    }

    pub fn remove_channel_scope(&self, channel_id: &str, thread_ids: &[String]) -> io::Result<()> {
        let _ = (channel_id, thread_ids);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::Journal;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-scope-skills-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock drift")
                .as_nanos()
        ));
        path
    }

    fn test_store(root: &std::path::Path) -> io::Result<Arc<Store>> {
        let journal = Journal::open(root.join("data").join("journal.jsonl"))?;
        Store::open(journal).map_err(|err| io::Error::other(err.to_string()))
    }

    #[test]
    fn reconcile_does_not_project_skills_into_channel_or_thread_dirs() {
        let root = temp_path("noop");
        let store = test_store(&root).expect("store");
        let channel = store
            .create_channel("dojo".into(), Some("actor_alice".into()))
            .expect("channel");
        let root_message_id = store
            .append_message(
                "actor_alice".into(),
                format!("#{}", channel.id),
                proto::types::MessageKind::Human,
                "lesson".into(),
                Vec::new(),
                Vec::new(),
                proto::types::MessageIntent::Chat,
                proto::types::DeliveryPolicy::NotifyOnly,
                None,
                None,
                Vec::new(),
                proto::types::Meta::default(),
                None,
            )
            .expect("message")
            .id;
        let thread = store
            .create_thread(channel.id.clone(), "lesson".into(), root_message_id)
            .expect("thread");
        let manager =
            ScopeSkills::new(root.join("workspaces"), root.join("agents")).expect("manager");

        manager.reconcile(&store).expect("reconcile");

        assert!(!root
            .join("workspaces")
            .join("channel")
            .join(&channel.id)
            .join("skills")
            .exists());
        assert!(!root
            .join("workspaces")
            .join("thread")
            .join(&thread.id)
            .join("skills")
            .exists());
        std::fs::remove_dir_all(root).ok();
    }
}
