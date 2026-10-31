use br_graph_contract::{Event, Fields};
use br_runner_jobs::{CallerHandle, EndWait, Error as JobsError, Removal, WriteEnd, WriteResult};
use futures::FutureExt;
use uuid::Uuid;

use crate::error::Error;
use crate::outcome::Outcome;
use crate::signals::{Signals, Stop};

enum Heard {
    End(Result<br_runner_jobs::SyncEnd, br_runner_jobs::Error>),
    Create(WriteResult),
}

async fn create_ended(slot: &mut Option<WriteEnd>) -> WriteResult {
    match slot.as_mut() {
        Some(end) => end.await,
        None => std::future::pending().await,
    }
}

async fn hear(end_wait: &mut EndWait, create_end: &mut Option<WriteEnd>) -> Heard {
    tokio::select! {
        biased;
        end = end_wait => Heard::End(end),
        created = create_ended(create_end) => Heard::Create(created),
    }
}

fn hand_over_cancel_unless_dead(
    caller: &CallerHandle,
    child: Uuid,
    create_end: &mut Option<WriteEnd>,
) {
    let dead = create_end
        .as_mut()
        .is_some_and(|end| matches!(end.now_or_never(), Some(Err(_))));
    if !dead {
        let _ = caller.cancel(child, Uuid::now_v7());
    }
}

fn removed(removal: Removal) -> Error {
    match removal {
        Removal::Failed(error) => Error::CreateRemoved {
            what: "the delivery queue gave it up after a failure".to_owned(),
            source: Some(Box::new(error)),
        },
        Removal::Refused(error) => Error::CreateRemoved {
            what: "the delivery queue refused it with an error that is no refusal".to_owned(),
            source: Some(Box::new(error)),
        },
        Removal::DependsOnRemoved => Error::CreateRemoved {
            what: "the upload it depends on was removed".to_owned(),
            source: None,
        },
        Removal::LeftUnsent => Error::CreateRemoved {
            what: "the delivery queue left it unsent".to_owned(),
            source: None,
        },
        _ => Error::CreateRemoved {
            what: "the delivery queue removed it".to_owned(),
            source: None,
        },
    }
}

pub(crate) async fn wait<F: Fields>(
    caller: &CallerHandle,
    child: Uuid,
    mut end_wait: EndWait,
    create_end: WriteEnd,
    signals: &mut Signals,
) -> Result<Outcome, Stop<F>> {
    let mut create_end = Some(create_end);
    loop {
        let heard = match signals.guard(hear(&mut end_wait, &mut create_end)).await {
            Ok(heard) => heard,
            Err(Stop::Event(Event::Cancelled)) => {
                hand_over_cancel_unless_dead(caller, child, &mut create_end);
                return Err(Stop::Event(Event::Cancelled));
            }
            Err(other) => return Err(other),
        };
        match heard {
            Heard::End(Ok(end)) => {
                return Outcome::from_end(end).ok_or_else(|| Stop::failed(Error::UnknownEnd));
            }
            Heard::End(Err(error)) => {
                hand_over_cancel_unless_dead(caller, child, &mut create_end);
                return Err(Stop::failed(Error::EndWait(Box::new(error))));
            }
            Heard::Create(Ok(())) => create_end = None,
            Heard::Create(Err(Removal::Refused(JobsError::Refused(refusal)))) => {
                return Ok(Outcome::from_refusal(refusal));
            }
            Heard::Create(Err(removal)) => {
                return Err(Stop::failed(removed(removal)));
            }
        }
    }
}
