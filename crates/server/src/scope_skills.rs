use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use proto::types::{Channel, ScopeKind, Thread};

use crate::store::Store;

pub struct ScopeSkills {
    root: PathBuf,
    source: FileActorSkillSource,
}

impl ScopeSkills {
    pub fn new(root: PathBuf, agents_root: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(&agents_root)?;
        Ok(Self {
            root,
            source: FileActorSkillSource { agents_root },
        })
    }

    pub fn reconcile(&self, store: &Arc<Store>) -> io::Result<()> {
        let channels = store.list_channels();
        let channel_ids = channels
            .iter()
            .map(|channel| channel.id.as_str())
            .collect::<HashSet<_>>();
        let threads = store.list_threads(None);
        let thread_ids = threads
            .iter()
            .map(|thread| thread.id.as_str())
            .collect::<HashSet<_>>();

        for channel in &channels {
            self.sync_channel_scope(channel)?;
        }
        for thread in &threads {
            self.sync_thread_channel_memberships(store, thread)?;
        }

        self.prune_missing_scope_skills(ScopeKind::Channel, &channel_ids)?;
        self.prune_missing_scope_skills(ScopeKind::Thread, &thread_ids)?;
        Ok(())
    }

    pub fn sync_actor_channel_membership(
        &self,
        store: &Arc<Store>,
        channel_id: &str,
        actor_id: &str,
    ) -> io::Result<()> {
        let Some(target) = self.source.target_for_actor(actor_id)? else {
            self.remove_actor_channel_membership(store, channel_id, actor_id)?;
            return Ok(());
        };
        self.ensure_actor_link(ScopeKind::Channel, channel_id, actor_id, &target)?;
        for thread in store.list_threads(Some(channel_id)) {
            self.ensure_actor_link(ScopeKind::Thread, &thread.id, actor_id, &target)?;
        }
        Ok(())
    }

    pub fn remove_actor_channel_membership(
        &self,
        store: &Arc<Store>,
        channel_id: &str,
        actor_id: &str,
    ) -> io::Result<()> {
        self.remove_actor_link(ScopeKind::Channel, channel_id, actor_id)?;
        for thread in store.list_threads(Some(channel_id)) {
            self.remove_actor_link(ScopeKind::Thread, &thread.id, actor_id)?;
        }
        Ok(())
    }

    pub fn sync_thread_channel_memberships(
        &self,
        store: &Arc<Store>,
        thread: &Thread,
    ) -> io::Result<()> {
        let Some(channel) = store.get_channel(&thread.channel_id) else {
            self.clear_scope_skills(ScopeKind::Thread, &thread.id)?;
            return Ok(());
        };
        let desired = self.desired_actor_targets(&channel)?;
        self.sync_scope_targets(ScopeKind::Thread, &thread.id, &desired)
    }

    pub fn remove_thread_scope(&self, thread_id: &str) -> io::Result<()> {
        self.clear_scope_skills(ScopeKind::Thread, thread_id)
    }

    pub fn remove_channel_scope(&self, channel_id: &str, thread_ids: &[String]) -> io::Result<()> {
        self.clear_scope_skills(ScopeKind::Channel, channel_id)?;
        for thread_id in thread_ids {
            self.clear_scope_skills(ScopeKind::Thread, thread_id)?;
        }
        Ok(())
    }

    fn sync_channel_scope(&self, channel: &Channel) -> io::Result<()> {
        let desired = self.desired_actor_targets(channel)?;
        self.sync_scope_targets(ScopeKind::Channel, &channel.id, &desired)
    }

    fn desired_actor_targets(&self, channel: &Channel) -> io::Result<BTreeMap<String, PathBuf>> {
        let mut desired = BTreeMap::new();
        for actor_id in &channel.members {
            if let Some(target) = self.target_for_actor_projection(actor_id)? {
                desired.insert(actor_id.clone(), target);
            }
        }
        Ok(desired)
    }

    fn target_for_actor_projection(&self, actor_id: &str) -> io::Result<Option<PathBuf>> {
        match self.source.target_for_actor(actor_id) {
            Ok(target) => Ok(target),
            Err(err) if err.kind() == io::ErrorKind::InvalidInput => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn sync_scope_targets(
        &self,
        kind: ScopeKind,
        scope_id: &str,
        desired: &BTreeMap<String, PathBuf>,
    ) -> io::Result<()> {
        let skills_dir = self.skills_dir(kind, scope_id)?;
        std::fs::create_dir_all(&skills_dir)?;

        let desired_ids = desired.keys().map(String::as_str).collect::<HashSet<_>>();
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for entry in entries {
                let entry = entry?;
                let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
                    continue;
                };
                if !desired_ids.contains(name.as_str()) {
                    remove_path_if_exists(&entry.path())?;
                }
            }
        }

        for (actor_id, target) in desired {
            self.ensure_actor_link(kind, scope_id, actor_id, target)?;
        }
        Ok(())
    }

    fn ensure_actor_link(
        &self,
        kind: ScopeKind,
        scope_id: &str,
        actor_id: &str,
        target: &Path,
    ) -> io::Result<()> {
        validate_path_component("actor_id", actor_id)?;
        let skills_dir = self.skills_dir(kind, scope_id)?;
        std::fs::create_dir_all(&skills_dir)?;
        let link_path = skills_dir.join(actor_id);
        match std::fs::read_link(&link_path) {
            Ok(existing) if existing == target => return Ok(()),
            Ok(_) => remove_path_if_exists(&link_path)?,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(_) => remove_path_if_exists(&link_path)?,
        }
        symlink_path(target, &link_path)
    }

    fn remove_actor_link(&self, kind: ScopeKind, scope_id: &str, actor_id: &str) -> io::Result<()> {
        validate_path_component("actor_id", actor_id)?;
        let link_path = self.skills_dir(kind, scope_id)?.join(actor_id);
        remove_path_if_exists(&link_path)
    }

    fn clear_scope_skills(&self, kind: ScopeKind, scope_id: &str) -> io::Result<()> {
        remove_path_if_exists(&self.skills_dir(kind, scope_id)?)
    }

    fn prune_missing_scope_skills(
        &self,
        kind: ScopeKind,
        live_ids: &HashSet<&str>,
    ) -> io::Result<()> {
        let root = self.root.join(scope_kind_name(kind));
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Ok(());
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(scope_id) = entry.file_name().to_str().map(ToOwned::to_owned) else {
                continue;
            };
            if !live_ids.contains(scope_id.as_str()) {
                remove_path_if_exists(&entry.path().join("skills"))?;
            }
        }
        Ok(())
    }

    fn skills_dir(&self, kind: ScopeKind, scope_id: &str) -> io::Result<PathBuf> {
        validate_path_component("scope_id", scope_id)?;
        Ok(self
            .root
            .join(scope_kind_name(kind))
            .join(scope_id)
            .join("skills"))
    }
}

/// Reject path components that could escape the projection root via
/// traversal sequences, separators, NUL, or platform-specific quirks.
/// Applied to every `actor_id` / `scope_id` before it is joined into a
/// filesystem path under the scope-skills root.
fn validate_path_component(label: &str, value: &str) -> io::Result<()> {
    proto::path_component::validate_path_component(value, label)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))
}

trait ActorSkillSource {
    fn target_for_actor(&self, actor_id: &str) -> io::Result<Option<PathBuf>>;
}

struct FileActorSkillSource {
    agents_root: PathBuf,
}

impl ActorSkillSource for FileActorSkillSource {
    fn target_for_actor(&self, actor_id: &str) -> io::Result<Option<PathBuf>> {
        validate_path_component("actor_id", actor_id)?;
        let release_path = self.agents_root.join(actor_id).join("bundle-release.json");
        let release_text = match std::fs::read_to_string(&release_path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let release: serde_json::Value =
            serde_json::from_str(&release_text).map_err(io::Error::other)?;
        let Some(source) = release.get("source").and_then(|v| v.as_str()) else {
            return Ok(None);
        };
        if source.is_empty() {
            return Ok(None);
        }
        Ok(Some(PathBuf::from(source)))
    }
}

fn scope_kind_name(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Channel => "channel",
        ScopeKind::Thread => "thread",
    }
}

fn remove_path_if_exists(path: &Path) -> io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if meta.file_type().is_symlink() || meta.is_file() {
        std::fs::remove_file(path)
    } else {
        std::fs::remove_dir_all(path)
    }
}

#[cfg(unix)]
fn symlink_path(source: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn symlink_path(source: &Path, target: &Path) -> io::Result<()> {
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, target)
    } else {
        std::os::windows::fs::symlink_file(source, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::journal::Journal;
    use crate::store::Store;

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

    fn write_bundle_release(
        agents_dir: &Path,
        actor_id: &str,
        bundle_source: &Path,
    ) -> io::Result<()> {
        std::fs::create_dir_all(bundle_source)?;
        std::fs::write(bundle_source.join("SKILL.md"), format!("# {actor_id}\n"))?;
        let actor_home = agents_dir.join(actor_id);
        std::fs::create_dir_all(&actor_home)?;
        std::fs::write(
            actor_home.join("bundle-release.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "actor": actor_id,
                "source": bundle_source.display().to_string(),
                "version": format!("{actor_id}-v1"),
                "installMode": "copy"
            }))?,
        )?;
        Ok(())
    }

    fn test_store(root: &Path, actor_specs: &[(&str, &Path)]) -> io::Result<Arc<Store>> {
        let data_dir = root.join("data");
        let agents_dir = root.join("agents");
        for (actor_id, bundle_source) in actor_specs {
            write_bundle_release(&agents_dir, actor_id, bundle_source)?;
        }
        let journal = Journal::open(data_dir.join("journal.jsonl"))?;
        Store::open(journal).map_err(|err| io::Error::other(err.to_string()))
    }

    fn create_thread_under(
        store: &Arc<Store>,
        channel_id: &str,
        actor_id: &str,
        title: &str,
    ) -> proto::types::Thread {
        let root_message_id = store
            .append_message(
                actor_id.into(),
                format!("#{channel_id}"),
                proto::types::MessageKind::Human,
                title.into(),
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
            .expect("append root message")
            .id;
        store
            .create_thread(channel_id.into(), title.into(), root_message_id)
            .expect("thread")
    }

    #[test]
    fn reconcile_links_channel_members_into_channel_and_thread_skills() {
        let root = temp_path("reconcile");
        let bundle_root = root.join("published").join("actor_alice");
        let store = test_store(&root, &[("actor_alice", bundle_root.as_path())]).expect("store");
        let channel = store
            .create_channel("dojo".into(), Some("actor_alice".into()))
            .expect("channel");
        let thread = create_thread_under(&store, &channel.id, "actor_alice", "lesson");
        let manager = ScopeSkills::new(root.join("data").join("workspaces"), root.join("agents"))
            .expect("manager");

        manager.reconcile(&store).expect("reconcile");

        let channel_link = root
            .join("data")
            .join("workspaces")
            .join("channel")
            .join(&channel.id)
            .join("skills")
            .join("actor_alice");
        let thread_link = root
            .join("data")
            .join("workspaces")
            .join("thread")
            .join(&thread.id)
            .join("skills")
            .join("actor_alice");
        assert_eq!(
            std::fs::read_link(&channel_link).expect("channel link"),
            bundle_root
        );
        assert_eq!(
            std::fs::read_link(&thread_link).expect("thread link"),
            bundle_root
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reconcile_skips_invalid_historical_actor_ids() {
        let root = temp_path("reconcile-invalid-actor");
        let bundle_root = root.join("published").join("actor_alice");
        let store = test_store(&root, &[("actor_alice", bundle_root.as_path())]).expect("store");
        let channel = store
            .create_channel("dojo".into(), Some("actor_alice".into()))
            .expect("channel");
        let invalid_actor_id =
            "actor_agent_machine_abbb0e0b_actor_human_local_ws_abbb0e0b_45b7a479";
        store
            .grant_channel(&channel.id, invalid_actor_id)
            .expect("grant invalid historical member");
        let manager = ScopeSkills::new(root.join("data").join("workspaces"), root.join("agents"))
            .expect("manager");

        manager.reconcile(&store).expect("reconcile");

        let channel_skills = root
            .join("data")
            .join("workspaces")
            .join("channel")
            .join(&channel.id)
            .join("skills");
        assert!(channel_skills.join("actor_alice").exists());
        assert!(!channel_skills.join(invalid_actor_id).exists());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn membership_sync_and_revoke_update_existing_thread_links() {
        let root = temp_path("membership");
        let alice_bundle = root.join("published").join("actor_alice");
        let bob_bundle = root.join("published").join("actor_bob");
        let store = test_store(
            &root,
            &[
                ("actor_alice", alice_bundle.as_path()),
                ("actor_bob", bob_bundle.as_path()),
            ],
        )
        .expect("store");
        let channel = store
            .create_channel("dojo".into(), Some("actor_alice".into()))
            .expect("channel");
        let thread = create_thread_under(&store, &channel.id, "actor_alice", "lesson");
        let manager = ScopeSkills::new(root.join("data").join("workspaces"), root.join("agents"))
            .expect("manager");

        manager.reconcile(&store).expect("reconcile");
        store
            .grant_channel(&channel.id, "actor_bob")
            .expect("grant");
        manager
            .sync_actor_channel_membership(&store, &channel.id, "actor_bob")
            .expect("sync bob");

        let bob_channel_link = root
            .join("data")
            .join("workspaces")
            .join("channel")
            .join(&channel.id)
            .join("skills")
            .join("actor_bob");
        let bob_thread_link = root
            .join("data")
            .join("workspaces")
            .join("thread")
            .join(&thread.id)
            .join("skills")
            .join("actor_bob");
        assert_eq!(
            std::fs::read_link(&bob_channel_link).expect("bob channel link"),
            bob_bundle
        );
        assert_eq!(
            std::fs::read_link(&bob_thread_link).expect("bob thread link"),
            bob_bundle
        );

        store
            .revoke_channel(&channel.id, "actor_bob")
            .expect("revoke");
        manager
            .remove_actor_channel_membership(&store, &channel.id, "actor_bob")
            .expect("remove bob");
        assert!(!bob_channel_link.exists());
        assert!(!bob_thread_link.exists());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reconcile_prunes_deleted_scope_skills() {
        let root = temp_path("prune");
        let bundle_root = root.join("published").join("actor_alice");
        let store = test_store(&root, &[("actor_alice", bundle_root.as_path())]).expect("store");
        let channel = store
            .create_channel("dojo".into(), Some("actor_alice".into()))
            .expect("channel");
        let thread = create_thread_under(&store, &channel.id, "actor_alice", "lesson");
        let manager = ScopeSkills::new(root.join("data").join("workspaces"), root.join("agents"))
            .expect("manager");

        manager.reconcile(&store).expect("reconcile");
        store.delete_thread(&thread.id).expect("delete thread");
        store
            .delete_channel(&channel.id, true)
            .expect("delete channel");

        manager.reconcile(&store).expect("reconcile prune");

        assert!(!root
            .join("data")
            .join("workspaces")
            .join("channel")
            .join(&channel.id)
            .join("skills")
            .exists());
        assert!(!root
            .join("data")
            .join("workspaces")
            .join("thread")
            .join(&thread.id)
            .join("skills")
            .exists());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rejects_path_traversal_components() {
        for (label, bad) in [
            ("actor_id", ".."),
            ("actor_id", "."),
            ("actor_id", ""),
            ("actor_id", "../etc"),
            ("actor_id", "a/b"),
            ("actor_id", "a\\b"),
            ("actor_id", "a\0b"),
            ("scope_id", "../../escape"),
        ] {
            let err = validate_path_component(label, bad)
                .expect_err(&format!("expected rejection for {label}={bad:?}"));
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        }

        for ok in ["actor_alice", "actor_42", "thread_abc", "chan_31f8fa85d909"] {
            validate_path_component("actor_id", ok).expect("legit id");
        }
    }

    #[test]
    fn ensure_actor_link_rejects_traversal() {
        let root = temp_path("traversal");
        let agents_root = root.join("agents");
        std::fs::create_dir_all(&agents_root).expect("agents root");
        let manager =
            ScopeSkills::new(root.join("workspaces"), agents_root.clone()).expect("manager");
        let target = agents_root.join("legit").join("bundle");
        std::fs::create_dir_all(&target).expect("target");

        let err = manager
            .ensure_actor_link(ScopeKind::Channel, "chan_legit", "../escape", &target)
            .expect_err("must reject ../");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        let err = manager
            .ensure_actor_link(ScopeKind::Thread, "../etc", "actor_alice", &target)
            .expect_err("must reject ../ scope");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        std::fs::remove_dir_all(root).ok();
    }
}
