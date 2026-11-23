mod input;
mod live;
#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::{Arc, Weak};

use uuid::Uuid;

use crate::backend::BoxFuture;
use crate::endwait::{EndStore, EndWait};
use crate::error::Error;
use crate::model::{Content, ResultRef};
use crate::queue::{QueueWriter, WriteEnd};
use crate::work::WorkCore;

pub use input::CreateChildInput;

pub(crate) trait CallerPort: Send + Sync {
    fn upload_payload(&self, job_id: Uuid, content: Content) -> Result<WriteEnd, Error>;

    fn create_child(&self, input: CreateChildInput) -> Result<WriteEnd, Error>;

    fn read_result(&self, result: ResultRef, keep: bool) -> BoxFuture<'_, Result<Content, Error>>;

    fn finish(&self, job_id: Uuid, command_id: Uuid) -> Result<WriteEnd, Error>;

    fn cancel(&self, job_id: Uuid, command_id: Uuid) -> Result<WriteEnd, Error>;

    fn end_wait(&self, child_job_id: Uuid) -> EndWait;
}

#[derive(Clone)]
pub struct CallerHandle {
    port: Arc<dyn CallerPort>,
}

impl CallerHandle {
    pub(crate) fn live(core: Arc<WorkCore>, writer: QueueWriter, ends: Weak<EndStore>) -> Self {
        Self::from_port(Arc::new(live::Live { core, writer, ends }))
    }

    pub(crate) fn from_port(port: Arc<dyn CallerPort>) -> Self {
        Self { port }
    }

    pub fn upload_payload(&self, job_id: Uuid, content: Content) -> Result<WriteEnd, Error> {
        self.port.upload_payload(job_id, content)
    }

    pub fn create_child(&self, input: CreateChildInput) -> Result<WriteEnd, Error> {
        self.port.create_child(input)
    }

    pub async fn read_result(&self, result: ResultRef, keep: bool) -> Result<Content, Error> {
        self.port.read_result(result, keep).await
    }

    pub fn finish(&self, job_id: Uuid, command_id: Uuid) -> Result<WriteEnd, Error> {
        self.port.finish(job_id, command_id)
    }

    pub fn cancel(&self, job_id: Uuid, command_id: Uuid) -> Result<WriteEnd, Error> {
        self.port.cancel(job_id, command_id)
    }

    pub fn end_wait(&self, child_job_id: Uuid) -> EndWait {
        self.port.end_wait(child_job_id)
    }
}

impl fmt::Debug for CallerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallerHandle").finish_non_exhaustive()
    }
}

impl fmt::Display for CallerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a caller handle")
    }
}
