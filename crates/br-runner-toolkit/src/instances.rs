use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use br_agentic_contract::{PortError, PortFuture};
use br_graph_contract::InstanceId;
use br_runner_jobs::{AgentBook, BookLookup};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::context::RunContext;
use crate::error::{Error, ErrorCode};
use crate::record::{Phase, RunRecord};

#[derive(Default)]
pub(crate) struct Instances {
    by_id: Mutex<HashMap<InstanceId, Arc<RunRecord>>>,
    changed: Notify,
    started: AtomicBool,
}

impl Instances {
    pub(crate) fn mark_started(&self) {
        self.started.store(true, Ordering::SeqCst);
    }

    pub(crate) fn insert(&self, id: InstanceId, record: Arc<RunRecord>) {
        self.by_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, record);
        self.changed.notify_waiters();
    }

    pub(crate) fn remove_run(&self, run_id: Uuid) {
        self.by_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, record| record.run_id != run_id);
    }

    async fn find(&self, id: InstanceId) -> Result<Arc<RunRecord>, Error> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.started.load(Ordering::SeqCst) {
                return Err(Error::new(
                    ErrorCode::NotStarted,
                    "the instance lookup is called before the runner started",
                ));
            }
            let found = self
                .by_id
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&id)
                .cloned();
            match found {
                Some(record) if record.phase() == Phase::Live => return Ok(record),
                Some(_) => {
                    return Err(Error::new(
                        ErrorCode::InstanceEnded,
                        "the run of this instance has ended its executor",
                    ));
                }
                None => changed.await,
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
