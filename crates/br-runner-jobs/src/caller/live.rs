use std::sync::{Arc, Weak};

use uuid::Uuid;

use super::{CallerPort, CreateChildInput};
use crate::backend::{BoxFuture, CreateChild};
use crate::endwait::{EndStore, EndWait};
use crate::error::Error;
use crate::graphql;
use crate::model::{Content, ResultRef};
use crate::queue::{QueueWriter, Write, WriteEnd};
use crate::work::WorkCore;

pub(super) struct Live {
    pub core: Arc<WorkCore>,
    pub writer: QueueWriter,
    pub ends: Weak<EndStore>,
}

impl CallerPort for Live {
    fn upload_payload(&self, job_id: Uuid, content: Content) -> Result<WriteEnd, Error> {
        Ok(self.writer.hand_over(Write::Upload { job_id, content }))
    }

    fn create_child(&self, input: CreateChildInput) -> Result<WriteEnd, Error> {
        let core = &self.core;
        Ok(self.writer.hand_over(Write::Create(CreateChild {
            command_id: input.command_id,
            job_id: input.job_id,
            runner_type: input.runner_type,
            mode: input.mode,
            config: input.config,
            payload: input.payload,
            passport: core.passport_json.clone(),
            parent_job_id: core.job_id,
            initial_job_id: core.initial_job_id,
            run_id: core.run_id,
            max_attempts: input.max_attempts,
        })))
    }

    fn read_result(&self, result: ResultRef, keep: bool) -> BoxFuture<'_, Result<Content, Error>> {
        Box::pin(async move {
            graphql::read_result(self.core.backend.as_ref(), &self.core.ids(), result, keep).await
        })
    }

    fn finish(&self, job_id: Uuid, command_id: Uuid) -> Result<WriteEnd, Error> {
        Ok(self.writer.hand_over(Write::Finish {
            run_id: self.core.run_id,
            job_id,
            command_id,
        }))
    }

    fn cancel(&self, job_id: Uuid, command_id: Uuid) -> Result<WriteEnd, Error> {
        Ok(self.writer.hand_over(Write::Cancel {
            run_id: self.core.run_id,
            job_id,
            command_id,
        }))
    }

    fn end_wait(&self, child_job_id: Uuid) -> EndWait {
        EndWait::watching_weak(&self.ends, child_job_id)
    }
}
