use std::sync::Arc;

use br_agentic_contract::StreamSink;
use br_llm_messages::{DeltaOp, ResponseDelta};

use super::Shared;
use crate::backend::BoxFuture;
use crate::error::Error;

pub struct ResponseSink {
    shared: Arc<Shared>,
}

impl ResponseSink {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

impl StreamSink for ResponseSink {
    fn send<'a>(&'a mut self, delta: ResponseDelta) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let shared = &self.shared;
            let mut state = shared.state.lock().await;
            if state.done || shared.kept().is_some() {
                return;
            }
            if matches!(delta.op(), DeltaOp::ResponseCompleted { .. }) {
                if let Err(error) = state.draft.clone().apply(&delta) {
                    shared.keep(Error::library(&error));
                }
                return;
            }
            if let Err(error) = state.draft.apply(&delta) {
                shared.keep(Error::library(&error));
                return;
            }
            match shared.lane.send_delta(&delta).await {
                Ok(()) => state.sent = true,
                Err(error) => shared.keep(error),
            }
        })
    }
}
