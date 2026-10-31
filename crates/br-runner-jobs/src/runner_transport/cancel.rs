use std::collections::VecDeque;

use async_nats::jetstream::kv::{Entry, Operation, Store, Watch};
use contract_jobs::runner::{CANCEL_BUCKET, CancelRun};
use contract_jobs::runner_transport::run_cancel_key;
use futures::StreamExt;
use uuid::Uuid;

use super::{Live, fail};
use crate::backend::{BoxFuture, RawFeed};
use crate::error::Error;
use crate::model::CancelEvent;

const ALL_KEYS: &str = ">";

struct CancelFeed {
    watch: Watch,
    store: Store,
    pending: VecDeque<Result<CancelEvent, Error>>,
    replayed: bool,
    ended: Option<Error>,
}

fn run_of(key: &str) -> Option<Uuid> {
    let run_id = Uuid::parse_str(key).ok()?;
    (run_cancel_key(run_id) == key).then_some(run_id)
}

impl CancelFeed {
    fn order(&mut self, run_id: Uuid, entry: &Entry) {
        match entry.operation {
            Operation::Put => match serde_json::from_slice::<CancelRun>(&entry.value) {
                Ok(order) if order.run_id == run_id => {
                    self.pending
                        .push_back(Ok(CancelEvent::OrderMade { run_id }));
                }
                Ok(_) => self.pending.push_back(Err(Error::Unreadable(format!(
                    "the cancel order of the key {run_id} names another Run"
                )))),
                Err(e) => self.pending.push_back(Err(Error::Unreadable(format!(
                    "the cancel order of the key {run_id}: {e}"
                )))),
            },
            Operation::Delete | Operation::Purge => {
                if self.replayed {
                    self.pending
                        .push_back(Ok(CancelEvent::OrderDeleted { run_id }));
                }
            }
        }
    }

    fn take(&mut self, entry: Entry) {
        if let Some(run_id) = run_of(&entry.key) {
            self.order(run_id, &entry);
        }
        if !self.replayed && entry.seen_current {
            self.replayed = true;
            self.pending.push_back(Ok(CancelEvent::ReplayComplete));
        }
    }
}

async fn failed(store: &Store, text: String) -> Error {
    let object = format!("the bucket {CANCEL_BUCKET}");
    let mut stream = store.stream.clone();
    match stream.info().await {
        Err(e) if fail::classify(&e) == fail::Class::Missing => fail::missing(&object),
        _ => Error::lost(format!("the cancel watch failed: {text}")),
    }
}

impl RawFeed<CancelEvent> for CancelFeed {
    fn next(&mut self) -> BoxFuture<'_, Result<CancelEvent, Error>> {
        Box::pin(async move {
            loop {
                if let Some(event) = self.pending.pop_front() {
                    return event;
                }
                if let Some(error) = &self.ended {
                    return Err(error.clone());
                }
                match self.watch.next().await {
                    Some(Ok(entry)) => self.take(entry),
                    Some(Err(e)) => {
                        let error = failed(&self.store, e.to_string()).await;
                        self.ended = Some(error);
                    }
                    None => self.ended = Some(Error::lost("the cancel watch ended")),
                }
            }
        })
    }
}

impl Live {
    pub(super) async fn open_cancel_feed(&self) -> Result<Box<dyn RawFeed<CancelEvent>>, Error> {
        let object = format!("the bucket {CANCEL_BUCKET}");
        let watch = tokio::time::timeout(self.reply_wait, self.cancel.watch_with_history(ALL_KEYS))
            .await
            .map_err(|_| {
                Error::Unavailable(crate::error::Failure::wait(
                    "the cancel bucket did not answer",
                ))
            })?
            .map_err(|e| fail::unsent(&object, &e))?;
        let mut stream = self.cancel.stream.clone();
        let stored = tokio::time::timeout(self.reply_wait, stream.info())
            .await
            .map_err(|_| {
                Error::Unavailable(crate::error::Failure::wait(
                    "the cancel bucket did not answer",
                ))
            })?
            .map_err(|e| fail::unsent(&object, &e))?
            .state
            .messages;
        let mut feed = CancelFeed {
            watch,
            store: self.cancel.clone(),
            pending: VecDeque::new(),
            replayed: false,
            ended: None,
        };
        if stored == 0 {
            feed.replayed = true;
            feed.pending.push_back(Ok(CancelEvent::ReplayComplete));
        }
        Ok(Box::new(feed))
    }
}
