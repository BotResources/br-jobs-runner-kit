use std::sync::Arc;

use br_agentic_contract::StreamSink;
use br_llm_messages::{DeltaOp, ResponseDelta};
use tokio::sync::OwnedMutexGuard;

use super::{OrderGuard, Shared, State};
use crate::backend::BoxFuture;
use crate::detach::detach;
use crate::error::Error;

pub struct ResponseSink {
    shared: Arc<Shared>,
}

impl ResponseSink {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

async fn deliver(shared: Arc<Shared>, mut state: OwnedMutexGuard<State>, delta: ResponseDelta) {
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
}

async fn held(order: OrderGuard, work: impl std::future::Future<Output = ()> + Send + 'static) {
    let _ = detach(async move {
        let _order = order;
        work.await;
    })
    .await;
}

impl StreamSink for ResponseSink {
    fn send<'a>(&'a mut self, delta: ResponseDelta) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let shared = Arc::clone(&self.shared);
            let order = shared.lane.order().await;
            let state = Arc::clone(&shared.state).lock_owned().await;
            let work = deliver(shared, state, delta);
            match order {
                Some(order) => held(order, work).await,
                None => work.await,
            }
        })
    }
}
