use std::sync::Arc;

use br_llm_messages::{DeltaOp, InterruptionReason, ModelResponse, ResponseDelta};

use super::{Shared, whole_response_deltas};
use crate::error::Error;

pub struct ResponseControl {
    shared: Arc<Shared>,
}

impl ResponseControl {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    pub fn failure(&self) -> Option<Error> {
        self.shared.kept()
    }

    pub async fn complete(&self, response: &ModelResponse) -> Result<(), Error> {
        let shared = &self.shared;
        let mut state = shared.state.lock().await;
        if state.done {
            return Err(Error::invalid(
                "the response is already completed or discarded",
            ));
        }
        if let Some(kept @ (Error::StreamBroken | Error::StreamClosed | Error::Store(_))) =
            shared.kept()
        {
            return Err(kept);
        }
        let deltas = whole_response_deltas(&state.draft, response)?;
        for delta in &deltas {
            match shared.lane.send_delta(delta).await {
                Ok(()) => {
                    state.sent = true;
                    state.draft.apply(delta).map_err(|e| Error::library(&e))?;
                }
                Err(error) => {
                    shared.keep(error.clone());
                    return Err(error);
                }
            }
        }
        state.done = true;
        Ok(())
    }

    pub async fn discard(&self) -> Result<(), Error> {
        let shared = &self.shared;
        let mut state = shared.state.lock().await;
        if state.done || !state.sent {
            return Ok(());
        }
        if matches!(
            shared.kept(),
            Some(Error::StreamBroken | Error::StreamClosed)
        ) {
            return Ok(());
        }
        let delta = ResponseDelta::new(
            state.draft.turn(),
            state.draft.entry(),
            DeltaOp::response_interrupted(InterruptionReason::cancelled()),
        );
        match shared.lane.send_delta(&delta).await {
            Ok(()) => {
                state.done = true;
                Ok(())
            }
            Err(Error::StreamClosed) => {
                shared.keep(Error::StreamClosed);
                Ok(())
            }
            Err(error) => {
                shared.keep(error.clone());
                Err(error)
            }
        }
    }

    pub(crate) async fn end(&self) {
        self.shared.state.lock().await.done = true;
    }
}
