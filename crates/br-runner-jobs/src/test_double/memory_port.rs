use std::sync::Arc;

use uuid::Uuid;

use super::memory_caller::{Answer, MemoryCall, Shared, WriteCall};
use crate::backend::BoxFuture;
use crate::caller::{CallerPort, CreateChildInput};
use crate::endwait::EndWait;
use crate::error::Error;
use crate::model::{Content, ResultRef};
use crate::queue::WriteEnd;

pub(super) struct MemoryPort {
    pub shared: Arc<Shared>,
}

impl MemoryPort {
    fn write(&self, call: MemoryCall, kind: WriteCall) -> Result<WriteEnd, Error> {
        let mut script = self.shared.script();
        script.calls.push(call);
        let answer = script
            .writes
            .get_mut(&kind)
            .and_then(std::collections::VecDeque::pop_front)
            .unwrap_or_else(Answer::delivered);
        match answer {
            Answer::Now(result) => Ok(WriteEnd::ended(result)),
            Answer::Late(result) => {
                let (sender, end) = WriteEnd::pair();
                script.held_writes.push((sender, result));
                Ok(end)
            }
            Answer::Never => {
                let (sender, end) = WriteEnd::pair();
                script.forever.push(sender);
                Ok(end)
            }
            Answer::Fail(error) => Err(error),
        }
    }
}

impl CallerPort for MemoryPort {
    fn upload_payload(&self, job_id: Uuid, content: Content) -> Result<WriteEnd, Error> {
        self.write(MemoryCall::Upload { job_id, content }, WriteCall::Upload)
    }

    fn create_child(&self, input: CreateChildInput) -> Result<WriteEnd, Error> {
        self.write(MemoryCall::Create(input), WriteCall::Create)
    }

    fn read_result(&self, result: ResultRef, keep: bool) -> BoxFuture<'_, Result<Content, Error>> {
        let (answer, late) = {
            let mut script = self.shared.script();
            script.calls.push(MemoryCall::ReadResult { result, keep });
            let answer = script
                .reads
                .pop_front()
                .unwrap_or_else(|| Answer::Fail(Error::invalid("no Result is scripted")));
            let late = match &answer {
                Answer::Late(_) => {
                    let (sender, receiver) = tokio::sync::oneshot::channel();
                    script.held_reads.push(sender);
                    Some(receiver)
                }
                _ => None,
            };
            (answer, late)
        };
        Box::pin(async move {
            match answer {
                Answer::Now(content) => Ok(content),
                Answer::Late(content) => {
                    if let Some(receiver) = late {
                        let _ = receiver.await;
                    }
                    Ok(content)
                }
                Answer::Never => std::future::pending().await,
                Answer::Fail(error) => Err(error),
            }
        })
    }

    fn finish(&self, job_id: Uuid, command_id: Uuid) -> Result<WriteEnd, Error> {
        self.write(MemoryCall::Finish { job_id, command_id }, WriteCall::Finish)
    }

    fn cancel(&self, job_id: Uuid, command_id: Uuid) -> Result<WriteEnd, Error> {
        self.write(MemoryCall::Cancel { job_id, command_id }, WriteCall::Cancel)
    }

    fn end_wait(&self, child_job_id: Uuid) -> EndWait {
        self.shared
            .script()
            .calls
            .push(MemoryCall::EndWait { child_job_id });
        match self.shared.store() {
            Some(store) => EndWait::watching(&store, child_job_id),
            None => EndWait::dropped(),
        }
    }
}
