use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use br_agentic_contract::{PortError, PortFuture};
use br_graph_contract::InstanceId;
use br_runner_jobs::{AgentBook, BookLookup};
use tokio::sync::watch;

use crate::context::RunContext;
use crate::error::{Error, ErrorCode};
use crate::record::{Phase, RunRecord};

pub(crate) struct Instances {
    by_id: Mutex<HashMap<InstanceId, Arc<RunRecord>>>,
    changed: watch::Sender<u64>,
    started: AtomicBool,
}

impl Default for Instances {
    fn default() -> Self {
        Self {
            by_id: Mutex::default(),
            changed: watch::channel(0).0,
            started: AtomicBool::new(false),
        }
    }
}

impl Instances {
    fn held(&self) -> MutexGuard<'_, HashMap<InstanceId, Arc<RunRecord>>> {
        self.by_id.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn mark_started(&self) {
        self.started.store(true, Ordering::SeqCst);
    }

    pub(crate) fn insert(&self, id: InstanceId, record: &Arc<RunRecord>) -> bool {
        let mut held = self.held();
        if matches!(record.phase(), Phase::Dropped | Phase::Aborted) {
            return false;
        }
        held.insert(id, Arc::clone(record));
        drop(held);
        self.changed.send_modify(|generation| *generation += 1);
        true
    }

    pub(crate) fn remove_run(&self, record: &Arc<RunRecord>, phase: Phase) {
        record.set_phase(phase);
        self.held().retain(|_, held| held.run_id != record.run_id);
    }

    #[cfg(test)]
    pub(crate) fn waiters(&self) -> usize {
        self.changed.receiver_count()
    }

    #[cfg(test)]
    pub(crate) fn holds(&self, id: InstanceId) -> bool {
        self.held().contains_key(&id)
    }

    async fn find(&self, id: InstanceId) -> Result<Arc<RunRecord>, Error> {
        let mut changes = self.changed.subscribe();
        loop {
            if !self.started.load(Ordering::SeqCst) {
                return Err(Error::new(
                    ErrorCode::NotStarted,
                    "the instance lookup is called before the runner started",
                ));
            }
            let found = self.held().get(&id).cloned();
            match found {
                Some(record) if record.phase() == Phase::Live => return Ok(record),
                Some(_) => {
                    return Err(Error::new(
                        ErrorCode::InstanceEnded,
                        "the run of this instance has ended its executor",
                    ));
                }
                None => {
                    let _ = changes.changed().await;
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct InstanceLookup {
    instances: Arc<Instances>,
}

impl InstanceLookup {
    pub(crate) fn new(instances: Arc<Instances>) -> Self {
        Self { instances }
    }

    pub async fn book(&self, instance: InstanceId) -> Result<Arc<AgentBook>, Error> {
        let record = self.instances.find(instance).await?;
        record.book().ok_or_else(|| {
            Error::new(
                ErrorCode::InstanceEnded,
                "the run of this instance holds no agent book",
            )
        })
    }

    pub async fn run_context(&self, instance: InstanceId) -> Result<RunContext, Error> {
        let record = self.instances.find(instance).await?;
        record.context().ok_or_else(|| {
            Error::new(
                ErrorCode::InstanceEnded,
                "the run of this instance holds no run context",
            )
        })
    }

    pub fn book_lookup(&self) -> Arc<dyn BookLookup> {
        Arc::new(self.clone())
    }
}

impl BookLookup for InstanceLookup {
    fn book<'a>(
        &'a self,
        instance: InstanceId,
    ) -> PortFuture<'a, Result<Arc<AgentBook>, PortError>> {
        Box::pin(async move {
            InstanceLookup::book(self, instance)
                .await
                .map_err(PortError::fault)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use br_runner_jobs::test_double::ScriptedJobs;

    use super::{InstanceLookup, Instances};
    use crate::error::ErrorCode;
    use crate::record::Phase;
    use crate::unit_support::{instance, record, runner_type, work};

    #[tokio::test(start_paused = true)]
    async fn given_a_lookup_that_is_dropped_while_it_waits_when_the_future_is_gone_then_no_waiter_is_left()
     {
        let instances = Arc::new(Instances::default());
        instances.mark_started();
        let lookup = InstanceLookup::new(Arc::clone(&instances));
        let waiting = {
            let lookup = lookup.clone();
            tokio::spawn(async move { lookup.book(instance(1)).await })
        };
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(instances.waiters(), 1);
        waiting.abort();
        let _ = waiting.await;
        assert_eq!(instances.waiters(), 0);
        let timed_out =
            tokio::time::timeout(Duration::from_secs(5), lookup.run_context(instance(2))).await;
        assert!(timed_out.is_err());
        assert_eq!(instances.waiters(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn given_a_waiting_lookup_when_the_instance_is_inserted_then_the_lookup_returns_the_record()
     {
        let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
        let instances = Arc::new(Instances::default());
        instances.mark_started();
        let work = work(&jobs, 1);
        let record = record(&work);
        let waiting = {
            let instances = Arc::clone(&instances);
            tokio::spawn(async move { instances.find(instance(1)).await.map(|r| r.run_id) })
        };
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(instances.insert(instance(1), &record));
        assert_eq!(waiting.await.expect("joined"), Ok(record.run_id));
    }

    #[tokio::test(start_paused = true)]
    async fn given_a_run_whose_executor_returned_when_its_instance_is_looked_up_then_the_error_is_instance_ended()
     {
        let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
        let instances = Arc::new(Instances::default());
        instances.mark_started();
        let work = work(&jobs, 1);
        let record = record(&work);
        assert!(instances.insert(instance(1), &record));
        record.set_phase(Phase::ExecutorReturned);
        let error = instances.find(instance(1)).await.err().expect("an error");
        assert_eq!(error.code(), ErrorCode::InstanceEnded);
    }

    #[tokio::test(start_paused = true)]
    async fn given_a_dropped_run_when_an_instance_is_inserted_then_it_is_refused_and_the_lookup_waits()
     {
        let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
        let instances = Arc::new(Instances::default());
        instances.mark_started();
        let work = work(&jobs, 1);
        let record = record(&work);
        instances.remove_run(&record, Phase::Dropped);
        assert!(!instances.insert(instance(1), &record));
        let timed_out =
            tokio::time::timeout(Duration::from_secs(5), instances.find(instance(1))).await;
        assert!(timed_out.is_err());
    }
}
