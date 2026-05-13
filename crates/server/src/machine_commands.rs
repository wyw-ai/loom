use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use proto::methods::MachineCommandResultParams;
use tokio::sync::oneshot;

#[derive(Default)]
pub struct MachineCommandBroker {
    pending: Mutex<HashMap<String, oneshot::Sender<MachineCommandResultParams>>>,
}

impl MachineCommandBroker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn register(&self, command_id: String) -> oneshot::Receiver<MachineCommandResultParams> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(command_id, tx);
        rx
    }

    pub fn complete(&self, result: MachineCommandResultParams) -> bool {
        self.pending
            .lock()
            .remove(&result.command_id)
            .is_some_and(|tx| tx.send(result).is_ok())
    }

    pub fn cancel(&self, command_id: &str) {
        self.pending.lock().remove(command_id);
    }
}
