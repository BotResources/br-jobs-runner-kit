use super::ThreadClient;
use crate::backend::Outcome;
use crate::closing::{Closing, ClosureKind};
use crate::error::Error;
use crate::ids::require_wait;
use crate::message::Confirmed;
use crate::stream::StreamHandle;
use crate::wire::{self, CloseParts};

fn confirmed(outcome: Outcome) -> Confirmed {
    Confirmed {
        command_id: outcome.command_id,
        status: outcome.status,
    }
}

impl ThreadClient {
    pub async fn close_message(
        &self,
        closing: &Closing,
        handle: Option<&StreamHandle>,
    ) -> Result<Confirmed, Error> {
        require_wait(closing.wait, "the outcome wait")?;
        closing.kind.check(closing.detail.as_deref())?;
        let parts = |seal| CloseParts {
            command_id: closing.command_id,
            thread_id: closing.thread_id,
            message_id: closing.message_id,
            kind: closing.kind,
            seal,
            acting_user_id: closing.acting_user_id,
        };
        let Some(handle) = handle else {
            if closing.kind == ClosureKind::Completed {
                return Err(Error::invalid(
                    "a Completed closing needs the handle of the stream",
                ));
            }
            let command = wire::closing(parts(None))?;
            return self
                .close_numbered(command, closing.wait)
                .await
                .map(confirmed);
        };
        if handle.message_id() != closing.message_id {
            return Err(Error::invalid("the handle belongs to another message"));
        }
        let core = &handle.core;
        let chunks = core.gate.lock().await;
        let completed = closing.kind == ClosureKind::Completed;
        core.judge_closing(completed)?;
        if completed && chunks.is_empty() {
            return Err(Error::invalid(
                "a Completed closing needs at least one chunk to seal",
            ));
        }
        let seal = if !chunks.is_empty() && (completed || core.healthy()) {
            Some(wire::seal_of(&chunks)?)
        } else {
            None
        };
        let command = wire::closing(parts(seal))?;
        let guard = core.begin_closing();
        drop(chunks);
        match self.close_numbered(command, closing.wait).await {
            Ok(outcome) => {
                guard.confirmed();
                Ok(confirmed(outcome))
            }
            Err(Error::Unavailable(failure)) => {
                guard.unsent();
                Err(Error::Unavailable(failure))
            }
            Err(Error::Refused(refusal)) => {
                guard.refused();
                Err(Error::Refused(refusal))
            }
            Err(other) => {
                guard.lost();
                Err(other)
            }
        }
    }
}
