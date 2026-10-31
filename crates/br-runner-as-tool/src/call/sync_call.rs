use br_agentic_contract::{CancelView, ToolContent};
use br_runner_jobs::{Content, EndWait, Error, ResultRef, SyncEnd, WriteEnd};
use futures::{FutureExt, select_biased};
use uuid::Uuid;

use super::content::{default_content, ended_text};
use super::prepare::Prepared;
use super::race::unless_cancelled;
use super::stop::refusal_text;
use super::{RunnerToolCode, Stop};
use crate::author::AuthorFailure;
use crate::error::{CreateRemoved, UnknownEnd};
use br_runner_jobs::Removal;

type Created = futures::future::Fuse<WriteEnd>;

impl RunnerToolCode {
    pub(super) async fn run_sync(
        &self,
        mut prepared: Prepared,
        cancel: &CancelView,
    ) -> Result<ToolContent, Stop> {
        let child = Uuid::now_v7();
        let wait = self.caller.end_wait(child);
        self.hand_upload(child, &mut prepared, cancel)?;
        let created = self.hand_create(child, prepared.form, cancel)?;
        let end = self.wait_for_end(child, wait, created, cancel).await?;
        self.settle(child, end, cancel).await
    }

    async fn wait_for_end(
        &self,
        child: Uuid,
        wait: EndWait,
        created: WriteEnd,
        cancel: &CancelView,
    ) -> Result<SyncEnd, Stop> {
        let mut end = wait.fuse();
        let mut created: Created = created.fuse();
        let mut triggered = cancel.triggered().fuse();
        loop {
            select_biased! {
                ended = end => {
                    return match ended {
                        Ok(end) => Ok(end),
                        Err(error) => {
                            self.cancel_unless_dead(child, &mut created);
                            Err(Stop::fault(error))
                        }
                    };
                },
                made = created => {
                    if let Err(removal) = made {
                        return Err(removed(removal));
                    }
                },
                () = triggered => {
                    self.cancel_unless_dead(child, &mut created);
                    return Err(Stop::Cancelled);
                },
            }
        }
    }

    async fn settle(
        &self,
        child: Uuid,
        end: SyncEnd,
        cancel: &CancelView,
    ) -> Result<ToolContent, Stop> {
        match end {
            SyncEnd::Completed { inline, result_ref } => {
                self.completed(child, inline, result_ref, cancel).await
            }
            SyncEnd::Failed { message, .. } => Err(Stop::Flag(ended_text(
                &self.runner_type,
                "failed",
                &message,
            ))),
            SyncEnd::Cancelled { message } => Err(Stop::Flag(ended_text(
                &self.runner_type,
                "cancelled",
                &message,
            ))),
            _ => Err(Stop::fault(UnknownEnd)),
        }
    }

    async fn completed(
        &self,
        child: Uuid,
        inline: Option<Content>,
        result_ref: Option<ResultRef>,
        cancel: &CancelView,
    ) -> Result<ToolContent, Stop> {
        let read = self.take_result(inline, result_ref, cancel).await;
        if matches!(read, Err(Stop::Cancelled)) || cancel.is_triggered() {
            return Err(Stop::Cancelled);
        }
        self.hand_finish(child);
        let result = read?;
        self.process(result, cancel).await
    }

    async fn take_result(
        &self,
        inline: Option<Content>,
        result_ref: Option<ResultRef>,
        cancel: &CancelView,
    ) -> Result<Content, Stop> {
        match (inline, result_ref) {
            (Some(content), _) => Ok(content),
            (None, Some(reference)) => {
                let read = unless_cancelled(cancel, self.caller.read_result(reference, self.keep))
                    .await
                    .ok_or(Stop::Cancelled)?;
                read.map_err(|error| read_stop(self, error))
            }
            (None, None) => Ok(Content {
                message: String::new(),
                attachments: Vec::new(),
            }),
        }
    }

    async fn process(&self, result: Content, cancel: &CancelView) -> Result<ToolContent, Stop> {
        let Some(processor) = &self.processor else {
            return default_content(&self.runner_type, &result);
        };
        let processed = unless_cancelled(cancel, processor(result))
            .await
            .ok_or(Stop::Cancelled)?;
        match processed {
            Ok(blocks) => Ok(ToolContent::new(blocks, false)),
            Err(AuthorFailure::Refused(text)) => Err(Stop::Flag(text)),
            Err(AuthorFailure::Fault(cause)) => Err(Stop::Fault(cause)),
        }
    }
}

fn removed(removal: Removal) -> Stop {
    match removal {
        Removal::Refused(error) => Stop::Flag(refusal_text(&error)),
        Removal::Failed(error) => Stop::fault(error),
        other => Stop::fault(CreateRemoved { removal: other }),
    }
}

fn read_stop(code: &RunnerToolCode, error: Error) -> Stop {
    match error {
        Error::Refused(_) => Stop::Flag(format!(
            "The Result read of the runner {} was refused: {}",
            code.runner_type,
            refusal_text(&error)
        )),
        other => Stop::fault(other),
    }
}
