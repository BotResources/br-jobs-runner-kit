use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use br_runner_jobs::{AgentBook, Arrival, ArrivalFeed, Error, Work};
use futures::FutureExt;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::panic::text_of;
use crate::record::RunRecord;
use crate::shared::Shared;

pub(crate) trait ArrivalSource: Send + 'static {
    fn next(&mut self) -> impl Future<Output = Result<Arrival, Error>> + Send;
}

pub(crate) trait ArrivalOpener: Send + Sync + 'static {
    type Source: ArrivalSource;

    fn open(&self) -> impl Future<Output = Result<Self::Source, Error>> + Send;
}

pub(crate) trait ArrivalSink: Send + Sync + 'static {
    fn arrival(&self, arrival: &Arrival);
    fn arrival_ended(&self, error: &Error);
    fn arrival_open_again(&self);
}

impl ArrivalSource for ArrivalFeed {
    fn next(&mut self) -> impl Future<Output = Result<Arrival, Error>> + Send {
        ArrivalFeed::next(self)
    }
}

pub(crate) struct WorkOpener(pub Arc<Work>);

impl ArrivalOpener for WorkOpener {
    type Source = ArrivalFeed;

    fn open(&self) -> impl Future<Output = Result<ArrivalFeed, Error>> + Send {
        self.0.arrival_feed()
    }
}

impl ArrivalSink for AgentBook {
    fn arrival(&self, arrival: &Arrival) {
        AgentBook::arrival(self, arrival);
    }

    fn arrival_ended(&self, error: &Error) {
        AgentBook::arrival_ended(self, error);
    }

    fn arrival_open_again(&self) {
        AgentBook::arrival_open_again(self);
    }
}

pub(crate) fn spawn(
    shared: Arc<Shared>,
    record: &Arc<RunRecord>,
    work: Arc<Work>,
    book: Arc<AgentBook>,
) -> JoinHandle<()> {
    tokio::spawn(keep(shared, record.run_id, WorkOpener(work), book))
}

pub(crate) async fn keep<O: ArrivalOpener, S: ArrivalSink>(
    shared: Arc<Shared>,
    run_id: Uuid,
    opener: O,
    sink: Arc<S>,
) {
    let mut was_cut = false;
    loop {
        let cycle = AssertUnwindSafe(one_feed(run_id, &opener, sink.as_ref(), &mut was_cut))
            .catch_unwind()
            .await;
        if let Err(payload) = cycle {
            let text = text_of(payload);
            tracing::error!(%run_id, panic = %text, "the arrival task of the run ended by a panic, the feed is opened again after the next beat");
            let ended = Error::InvalidInput(format!("the arrival task panicked: {text}"));
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| sink.arrival_ended(&ended))) {
                tracing::error!(%run_id, panic = %text_of(payload), "the end of the arrival feed could not be given to the agent book");
            }
            was_cut = true;
        }
        shared.next_beat().await;
    }
}

async fn one_feed<O: ArrivalOpener, S: ArrivalSink>(
    run_id: Uuid,
    opener: &O,
    sink: &S,
    was_cut: &mut bool,
) {
    let mut feed = match opener.open().await {
        Ok(feed) => feed,
        Err(error) => {
            tracing::error!(%run_id, %error, "the arrival feed of the run cannot be opened");
            sink.arrival_ended(&error);
            *was_cut = true;
            return;
        }
    };
    if *was_cut {
        sink.arrival_open_again();
        *was_cut = false;
    }
    loop {
        match feed.next().await {
            Ok(arrival) => {
                if let Arrival::Unreadable { error, .. } = &arrival {
                    tracing::error!(%run_id, %error, "an event of the arrival feed cannot be decoded");
                }
                sink.arrival(&arrival);
            }
            Err(error) => {
                tracing::error!(%run_id, %error, "the arrival feed of the run ended");
                sink.arrival_ended(&error);
                *was_cut = true;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
