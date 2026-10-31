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
                    if changes.changed().await.is_err() {
                        return Err(Error::new(ErrorCode::InstanceEnded, "the runner is gone"));
                    }
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
