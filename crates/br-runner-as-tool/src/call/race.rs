use std::future::Future;
use std::pin::pin;

use br_agentic_contract::CancelView;
use futures::future::{Either, select};

pub(super) async fn unless_cancelled<F: Future>(
    cancel: &CancelView,
    future: F,
) -> Option<F::Output> {
    let triggered = pin!(cancel.triggered());
    let future = pin!(future);
    match select(triggered, future).await {
        Either::Left(_) => None,
        Either::Right((output, _)) => Some(output),
    }
}
