use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::{PortError, PortFuture, ThreadRef};
use br_llm_messages::{AgentId, Author};
use uuid::Uuid;

use crate::queue::{QueueObserver, QueueStore};

#[derive(Debug, Clone)]
pub struct WriteContext {
    pub agent: AgentId,
    pub author: Author,
    pub job_id: Option<Uuid>,
    pub acting_user_id: Option<Uuid>,
    pub impersonator_id: Option<Uuid>,
}

type Function =
    dyn Fn(ThreadRef) -> PortFuture<'static, Result<WriteContext, PortError>> + Send + Sync;

#[derive(Clone)]
pub struct WriteContextFn(Arc<Function>);

impl WriteContextFn {
    pub fn new<F, Fut>(function: F) -> Self
    where
        F: Fn(ThreadRef) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<WriteContext, PortError>> + Send + 'static,
    {
        Self(Arc::new(move |thread| Box::pin(function(thread))))
    }

    pub(crate) fn call(
        &self,
        thread: ThreadRef,
    ) -> PortFuture<'static, Result<WriteContext, PortError>> {
        (self.0)(thread)
    }
}

pub struct ThreadAccessSettings {
    pub outcome_wait: Duration,
    pub retry_wait: Duration,
    pub store: Arc<dyn QueueStore>,
    pub observer: Arc<dyn QueueObserver>,
    pub write_context: WriteContextFn,
}
