use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use br_graph_contract::CancelToken;
use br_runner_jobs::{PresenceStatus, QueueObserver, TriggerFeed};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::calls::Calls;
use crate::closings::Closings;
use crate::hook::LifecycleHook;
#[cfg(feature = "test-host")]
use crate::host_state::HostState;
use crate::instances::Instances;
use crate::record::RunRecord;
use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    NoEntry,
    Push,
    Claim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoopState {
    pub mode: Mode,
    pub replay_complete: bool,
}

impl LoopState {
    pub(crate) fn can_ask(&self) -> bool {
        self.mode != Mode::NoEntry && self.replay_complete
    }
}

#[derive(Default)]
struct Registry {
    runs: HashMap<Uuid, Arc<RunRecord>>,
    known_cancels: HashSet<Uuid>,
}

pub(crate) struct TriggerFeedState {
    pub feed: Option<TriggerFeed>,
    pub closed_at_beat: Option<u64>,
}

pub(crate) struct Shared {
    pub calls: Calls,
    pub settings: Settings,
    pub hook: Option<LifecycleHook>,
    pub author_observer: Option<Arc<dyn QueueObserver>>,
    pub drain: CancellationToken,
    pub status: watch::Sender<PresenceStatus>,
    pub beat: watch::Sender<u64>,
    pub loop_state: watch::Sender<LoopState>,
    pub instances: Arc<Instances>,
    pub closings: Closings,
    pub triggers: tokio::sync::Mutex<TriggerFeedState>,
    mode_epoch: AtomicU64,
    stopped_epoch: AtomicU64,
    registry: Mutex<Registry>,
    #[cfg(feature = "test-host")]
    pub host: Option<Arc<HostState>>,
}

const NOT_STOPPED: u64 = u64::MAX;

impl Shared {
    pub(crate) fn new(
        calls: Calls,
        settings: Settings,
        hook: Option<LifecycleHook>,
        author_observer: Option<Arc<dyn QueueObserver>>,
        instances: Arc<Instances>,
    ) -> Self {
        Self {
            calls,
            settings,
            hook,
            author_observer,
            drain: CancellationToken::new(),
            status: watch::channel(PresenceStatus::Ready).0,
            beat: watch::channel(0).0,
            loop_state: watch::channel(LoopState {
                mode: Mode::NoEntry,
                replay_complete: false,
            })
            .0,
            instances,
            closings: Closings::default(),
            triggers: tokio::sync::Mutex::new(TriggerFeedState {
                feed: None,
                closed_at_beat: None,
            }),
            mode_epoch: AtomicU64::new(0),
            stopped_epoch: AtomicU64::new(NOT_STOPPED),
            registry: Mutex::new(Registry::default()),
            #[cfg(feature = "test-host")]
            host: None,
        }
    }

    fn registry(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn draining(&self) -> bool {
        self.drain.is_cancelled()
    }

    pub(crate) fn register_run(
        &self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> Result<(Arc<RunRecord>, CancelToken), Arc<RunRecord>> {
        let token = CancelToken::new();
        let record = RunRecord::new(run_id, job_id, token.handle().clone());
        let mut registry = self.registry();
        if let Some(existing) = registry.runs.get(&run_id) {
            return Err(Arc::clone(existing));
        }
        if registry.known_cancels.contains(&run_id) || self.host_cancelled() {
            token.handle().trigger();
        }
        registry.runs.insert(run_id, Arc::clone(&record));
        drop(registry);
        self.host_run_taken();
        Ok((record, token))
    }

    pub(crate) fn runs(&self) -> Vec<Arc<RunRecord>> {
        self.registry().runs.values().cloned().collect()
    }

    pub(crate) fn forget_run(&self, run_id: Uuid) {
        self.registry().runs.remove(&run_id);
        self.host_run_forgotten();
    }

    pub(crate) fn order_made(&self, run_id: Uuid) {
        let mut registry = self.registry();
        registry.known_cancels.insert(run_id);
        if let Some(record) = registry.runs.get(&run_id) {
            record.cancel.trigger();
        }
    }

    pub(crate) fn order_deleted(&self, run_id: Uuid) {
        self.registry().known_cancels.remove(&run_id);
    }

    pub(crate) fn replace_orders(&self, orders: HashSet<Uuid>) {
        let mut registry = self.registry();
        for run_id in &orders {
            if let Some(record) = registry.runs.get(run_id) {
                record.cancel.trigger();
            }
        }
        registry.known_cancels = orders;
    }

    pub(crate) fn mode_epoch(&self) -> u64 {
        self.mode_epoch.load(Ordering::SeqCst)
    }

    pub(crate) fn mode_changed(&self, mode: Mode) {
        self.mode_epoch.fetch_add(1, Ordering::SeqCst);
        self.loop_state.send_modify(|state| state.mode = mode);
    }

    pub(crate) fn stop_claims(&self, claim_epoch: u64) {
        if claim_epoch == self.mode_epoch() {
            self.stopped_epoch.store(claim_epoch, Ordering::SeqCst);
        }
    }

    pub(crate) fn claims_stopped(&self) -> bool {
        self.stopped_epoch.load(Ordering::SeqCst) == self.mode_epoch()
    }

    pub(crate) fn current_beat(&self) -> u64 {
        *self.beat.borrow()
    }

    pub(crate) fn beat_now(&self) {
        self.beat.send_modify(|beat| *beat += 1);
    }

    pub(crate) async fn next_beat(&self) {
        let mut beats = self.beat.subscribe();
        let _ = beats.changed().await;
    }
}

#[cfg(feature = "test-host")]
impl Shared {
    pub(crate) fn with_host(mut self, host: Option<Arc<HostState>>) -> Self {
        self.host = host;
        self
    }

    fn host_cancelled(&self) -> bool {
        self.host.as_ref().is_some_and(|host| host.cancelled())
    }

    fn host_run_taken(&self) {
        if let Some(host) = &self.host {
            host.run_taken();
        }
    }

    fn host_run_forgotten(&self) {
        if let Some(host) = &self.host {
            host.run_forgotten();
        }
    }

    pub(crate) fn host_has_taken_its_run(&self) -> bool {
        self.host
            .as_ref()
            .is_some_and(|host| host.has_taken_a_run())
    }

    pub(crate) fn cancel_all_runs(&self) {
        for record in self.runs() {
            record.cancel.trigger();
        }
    }
}

#[cfg(not(feature = "test-host"))]
impl Shared {
    fn host_cancelled(&self) -> bool {
        false
    }

    fn host_run_taken(&self) {}

    fn host_run_forgotten(&self) {}

    pub(crate) fn host_has_taken_its_run(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use br_runner_jobs::test_double::ScriptedJobs;
    use std::sync::Arc;

    use super::Mode;
    use crate::instances::Instances;
    use crate::unit_support::{id, shared};

    fn fresh() -> Arc<super::Shared> {
        let jobs = ScriptedJobs::new("analyst", "i1");
        shared(&jobs, None, &Arc::new(Instances::default()))
    }

    #[test]
    fn given_a_stop_decided_before_a_mode_change_when_it_is_applied_late_then_the_claims_are_not_stopped()
     {
        let shared = fresh();
        let epoch = shared.mode_epoch();
        shared.mode_changed(Mode::Claim);
        shared.stop_claims(epoch);
        assert!(!shared.claims_stopped());
    }

    #[test]
    fn given_a_stop_decided_in_the_current_mode_when_the_mode_changes_then_the_claims_are_open_again()
     {
        let shared = fresh();
        shared.mode_changed(Mode::Claim);
        shared.stop_claims(shared.mode_epoch());
        assert!(shared.claims_stopped());
        shared.mode_changed(Mode::Claim);
        assert!(!shared.claims_stopped());
    }

    #[test]
    fn given_an_order_made_and_deleted_before_the_run_exists_when_the_run_registers_then_it_starts_live()
     {
        let shared = fresh();
        shared.order_made(id(1));
        shared.order_deleted(id(1));
        let (_, token) = shared.register_run(id(1), id(2)).ok().expect("registered");
        assert!(!token.handle().is_triggered());
    }

    #[test]
    fn given_an_order_made_for_a_registered_run_when_the_order_is_deleted_then_the_token_stays_triggered()
     {
        let shared = fresh();
        let (_, token) = shared.register_run(id(1), id(2)).ok().expect("registered");
        shared.order_made(id(1));
        shared.order_deleted(id(1));
        assert!(token.handle().is_triggered());
    }

    #[test]
    fn given_an_order_made_before_the_run_exists_when_the_run_registers_then_it_starts_cancelled() {
        let shared = fresh();
        shared.order_made(id(1));
        let (_, token) = shared.register_run(id(1), id(2)).ok().expect("registered");
        assert!(token.handle().is_triggered());
    }

    #[test]
    fn given_a_run_already_registered_when_the_same_run_registers_again_then_the_first_record_is_returned()
     {
        let shared = fresh();
        let (first, _) = shared.register_run(id(1), id(2)).ok().expect("registered");
        let existing = shared.register_run(id(1), id(2)).err().expect("refused");
        assert!(Arc::ptr_eq(&first, &existing));
    }
}
