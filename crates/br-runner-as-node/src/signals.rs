use std::future::Future;

use br_graph_contract::{CancelView, Dropped, Event, EventSender, Fields, Triggered};

pub(crate) struct Signals {
    cancel: Triggered,
    dropped: Dropped,
}

pub(crate) enum Stop<F: Fields> {
    Gone,
    Event(Event<F>),
}

impl<F: Fields> Stop<F> {
    pub(crate) fn failed(cause: impl Into<br_graph_contract::Cause>) -> Self {
        Self::Event(Event::failed(cause))
    }
}

impl Signals {
    pub(crate) fn new<F: Fields>(cancel: &CancelView, sender: &EventSender<F>) -> Self {
        Self {
            cancel: cancel.triggered(),
            dropped: sender.dropped(),
        }
    }

    pub(crate) async fn guard<F: Fields, T>(
        &mut self,
        work: impl Future<Output = T>,
    ) -> Result<T, Stop<F>> {
        tokio::select! {
            biased;
            () = &mut self.dropped => Err(Stop::Gone),
            () = &mut self.cancel => Err(Stop::Event(Event::Cancelled)),
            done = work => Ok(done),
        }
    }
}
