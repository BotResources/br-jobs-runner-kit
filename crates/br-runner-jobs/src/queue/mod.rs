mod deps;
mod report;
mod sender;
mod shared;
mod task;
#[cfg(test)]
mod tests;
mod write;

use std::sync::Arc;

use serde_json::Value;
use uuid::Uuid;

use crate::backend::Fact;
use crate::error::Error;
use crate::model::{Content, Failed};

pub use report::{
    IgnoreReports, LastFailure, QueueObserver, QueueReport, QueueSettings, QueueState,
};
pub use shared::Gate;
pub use write::{Removal, WriteEnd, WriteKind, WriteName, WriteResult};

pub(crate) use sender::WriteSender;
pub(crate) use shared::Shared;
pub(crate) use write::Write;

#[derive(Clone)]
pub(crate) struct QueueWriter {
    pub(crate) shared: Arc<Shared>,
}

impl QueueWriter {
    pub(crate) fn hand_over(&self, write: Write) -> WriteEnd {
        self.shared.hand_over(write)
    }

    pub(crate) fn hold(
        &self,
        write: Write,
        follow: Option<Write>,
        gate: crate::backend::BoxFuture<'static, Gate>,
    ) -> WriteEnd {
        self.shared.hold(write, follow, gate)
    }
}

pub struct DeliveryQueue {
    writer: QueueWriter,
    run_id: Uuid,
}

impl DeliveryQueue {
    pub(crate) fn start(
        sender: Arc<dyn WriteSender>,
        settings: QueueSettings,
        run_id: Uuid,
    ) -> Result<(Self, WriteEnd), Error> {
        if settings.outcome_wait.is_zero() {
            return Err(Error::invalid("the outcome wait is zero"));
        }
        if settings.retry_wait.is_zero() {
            return Err(Error::invalid("the wait between attempts is zero"));
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return Err(Error::invalid("the delivery queue needs an async runtime"));
        };
        let start_id = settings.start_id;
        let shared = Shared::new(sender, settings);
        runtime.spawn(task::run(Arc::clone(&shared)));
        let queue = Self {
            writer: QueueWriter { shared },
            run_id,
        };
        let started = queue.writer.hand_over(Write::Started { run_id, start_id });
        Ok((queue, started))
    }

    pub(crate) fn writer(&self) -> QueueWriter {
        self.writer.clone()
    }

    pub fn answer(&self, answer: Content) -> WriteEnd {
        self.writer.hand_over(Write::Answer(answer))
    }

    pub fn saved_state(&self, state: Value) -> WriteEnd {
        self.writer.hand_over(Write::SavedState(state))
    }

    pub fn plan_declared(
        &self,
        steps: Vec<String>,
        declaration_id: Option<Uuid>,
    ) -> Result<WriteEnd, Error> {
        if steps.is_empty() {
            return Err(Error::invalid("a plan declares at least one step"));
        }
        Ok(self.writer.hand_over(Write::Fact(Fact::Plan {
            run_id: self.run_id,
            steps,
            declaration_id,
        })))
    }

    pub fn step_started(
        &self,
        index: u32,
        label: String,
        started_at: chrono::DateTime<chrono::Utc>,
    ) -> WriteEnd {
        self.writer.hand_over(Write::Fact(Fact::Step {
            run_id: self.run_id,
            index,
            label,
            started_at,
        }))
    }

    pub fn completed(&self) -> WriteEnd {
        self.writer.hand_over(Write::Fact(Fact::Completed {
            run_id: self.run_id,
        }))
    }

    pub fn failed(&self, failed: Failed) -> WriteEnd {
        self.writer.hand_over(Write::Fact(Fact::Failed {
            run_id: self.run_id,
            failed,
        }))
    }

    pub async fn drained(&self) -> QueueState {
        self.writer.shared.drained().await
    }

    pub fn state(&self) -> QueueState {
        self.writer.shared.snapshot()
    }
}

impl Drop for DeliveryQueue {
    fn drop(&mut self) {
        self.writer.shared.stop_now();
    }
}

impl std::fmt::Debug for DeliveryQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeliveryQueue").finish_non_exhaustive()
    }
}
