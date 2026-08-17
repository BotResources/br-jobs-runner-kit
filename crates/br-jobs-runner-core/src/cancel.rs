use std::collections::HashSet;

use uuid::Uuid;

#[derive(Debug, Default)]
pub struct CancelSet {
    cancelled: HashSet<Uuid>,
}

impl CancelSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_put(&mut self, run_id: Uuid) {
        self.cancelled.insert(run_id);
    }

    pub fn apply_delete(&mut self, run_id: Uuid) {
        self.cancelled.remove(&run_id);
    }

    pub fn replace_with_replay(&mut self, run_ids: impl IntoIterator<Item = Uuid>) {
        self.cancelled = run_ids.into_iter().collect();
    }

    pub fn is_cancelled(&self, run_id: Uuid) -> bool {
        self.cancelled.contains(&run_id)
    }

    pub fn len(&self) -> usize {
        self.cancelled.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cancelled.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_put_when_checking_then_cancelled() {
        let mut set = CancelSet::new();
        let run = Uuid::now_v7();
        assert!(!set.is_cancelled(run));
        set.apply_put(run);
        assert!(set.is_cancelled(run));
    }

    #[test]
    fn given_delete_when_checking_then_no_longer_cancelled() {
        let mut set = CancelSet::new();
        let run = Uuid::now_v7();
        set.apply_put(run);
        set.apply_delete(run);
        assert!(!set.is_cancelled(run));
        assert!(set.is_empty());
    }

    #[test]
    fn given_reconnect_replay_when_replacing_then_only_replayed_entries_remain() {
        let mut set = CancelSet::new();
        let stale = Uuid::now_v7();
        let replayed_a = Uuid::now_v7();
        let replayed_b = Uuid::now_v7();
        set.apply_put(stale);
        set.replace_with_replay([replayed_a, replayed_b]);
        assert!(!set.is_cancelled(stale));
        assert!(set.is_cancelled(replayed_a));
        assert!(set.is_cancelled(replayed_b));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn given_empty_replay_when_replacing_then_set_is_empty() {
        let mut set = CancelSet::new();
        set.apply_put(Uuid::now_v7());
        set.replace_with_replay([]);
        assert!(set.is_empty());
    }
}
