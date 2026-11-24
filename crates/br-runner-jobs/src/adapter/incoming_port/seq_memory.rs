use std::collections::HashMap;
use std::sync::Mutex;

use br_runner_jobs_client::UnreadItem;

use super::listed::Listed;
use uuid::Uuid;

#[derive(Default)]
pub(in crate::adapter) struct SeqMemory {
    by_agent: Mutex<HashMap<Uuid, HashMap<Uuid, i64>>>,
}

impl SeqMemory {
    pub(super) fn keep(&self, agent: Uuid, read: &[Listed]) {
        let seqs = read
            .iter()
            .map(|sequenced| (sequenced.item.item_id(), sequenced.seq))
            .collect();
        self.lock().insert(agent, seqs);
    }

    pub(super) fn drop_agent(&self, agent: Uuid) {
        self.lock().remove(&agent);
    }

    pub(super) fn sequence(&self, agent: Uuid, kept: Vec<UnreadItem>) -> Option<Vec<Listed>> {
        let guard = self.lock();
        let seqs = guard.get(&agent)?;
        kept.into_iter()
            .map(|item| {
                let seq = *seqs.get(&item.item_id())?;
                Some(Listed { seq, item })
            })
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, HashMap<Uuid, i64>>> {
        self.by_agent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
