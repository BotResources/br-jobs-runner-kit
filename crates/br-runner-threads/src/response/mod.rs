mod control;
mod sink;
mod whole;

use std::sync::{Arc, Mutex};

use br_llm_messages::{EntryId, ResponseDelta, ResponseDraft, TurnId};

use crate::backend::BoxFuture;
use crate::error::Error;

pub use control::ResponseControl;
pub use sink::ResponseSink;

pub(crate) use whole::whole_response_deltas;

pub(crate) trait DeltaLane: Send + Sync {
    fn send_delta<'a>(&'a self, delta: &'a ResponseDelta) -> BoxFuture<'a, Result<(), Error>>;
}

pub(crate) struct State {
    pub(crate) draft: ResponseDraft,
    pub(crate) sent: bool,
    pub(crate) done: bool,
}

pub(crate) struct Shared {
    pub(crate) state: tokio::sync::Mutex<State>,
    pub(crate) failure: Mutex<Option<Error>>,
    pub(crate) lane: Arc<dyn DeltaLane>,
}

impl Shared {
    pub(crate) fn keep(&self, error: Error) {
        let mut failure = self
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if failure.is_none() {
            *failure = Some(error);
        }
    }

    pub(crate) fn kept(&self) -> Option<Error> {
        self.failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

pub(crate) fn pair(
    turn: TurnId,
    entry: EntryId,
    lane: Arc<dyn DeltaLane>,
) -> (ResponseSink, ResponseControl) {
    let shared = Arc::new(Shared {
        state: tokio::sync::Mutex::new(State {
            draft: ResponseDraft::new(turn, entry),
            sent: false,
            done: false,
        }),
        failure: Mutex::new(None),
        lane,
    });
    (
        ResponseSink::new(Arc::clone(&shared)),
        ResponseControl::new(shared),
    )
}
