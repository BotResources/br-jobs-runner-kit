use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use br_util_nats_fabric::{
    Fabric, FabricError, KV_PUBLISHED_LANGUAGE, KvKey, KvPrefix, ProjectionError,
    PublishedLanguageConsumer, PublishedLanguageReader,
};
use contract_jobs::dispatch_mode::dispatch_mode_key;
use serde_json::Value;
use tokio::sync::mpsc;

use super::mode_sink::{ModeItem, ModeSink, SinkClosed, read};
use super::{Live, fail};
use crate::backend::{BoxFuture, RawFeed};
use crate::error::Error;
use crate::model::ModeEvent;

type Watching = Pin<Box<dyn Future<Output = Result<(), ProjectionError<SinkClosed>>> + Send>>;

struct ModeFeed {
    fabric: Fabric,
    reader: PublishedLanguageReader<Value>,
    key: KvKey,
    receiver: mpsc::UnboundedReceiver<ModeItem>,
    watch: Watching,
    pending: VecDeque<ModeItem>,
    last: Option<ModeEvent>,
    started: bool,
    ended: Option<Error>,
}

fn bucket_error(error: FabricError) -> Error {
    fail::fabric(&format!("the bucket {KV_PUBLISHED_LANGUAGE}"), error)
}

impl ModeFeed {
    fn push(&mut self, item: ModeItem) {
        match item {
            Ok(event) if self.last == Some(event) => {}
            Ok(event) => {
                self.last = Some(event);
                self.pending.push_back(Ok(event));
            }
            Err(error) => {
                self.last = None;
                self.pending.push_back(Err(error));
            }
        }
    }

    fn drain(&mut self) {
        while let Ok(item) = self.receiver.try_recv() {
            self.push(item);
        }
    }

    async fn start(&mut self) {
        if let std::task::Poll::Ready(outcome) = futures::poll!(self.watch.as_mut()) {
            self.watch_ended(outcome).await;
            return;
        }
        let initial = match self.reader.get(&self.key).await {
            Ok(Some(value)) => read(&value),
            Ok(None) => Ok(ModeEvent::NoEntry),
            Err(FabricError::Decode { subject, detail }) => {
                Err(Error::Unreadable(format!("{subject}: {detail}")))
            }
            Err(error) => {
                self.ended = Some(bucket_error(error));
                return;
            }
        };
        self.started = true;
        self.push(initial);
    }

    async fn watch_ended(&mut self, outcome: Result<(), ProjectionError<SinkClosed>>) {
        self.drain();
        self.ended = Some(match outcome {
            Ok(()) => Error::lost("the dispatch mode watch ended"),
            Err(ProjectionError::Fabric(FabricError::Decode { subject, detail })) => {
                self.pending
                    .push_back(Err(Error::Unreadable(format!("{subject}: {detail}"))));
                Error::lost("the dispatch mode watch ended on an entry that is not JSON")
            }
            Err(ProjectionError::Fabric(error)) => {
                match PublishedLanguageReader::<Value>::open(&self.fabric).await {
                    Err(missing) => bucket_error(missing),
                    Ok(_) => bucket_error(error),
                }
            }
            Err(ProjectionError::Sink(closed)) => Error::lost(closed.to_string()),
        });
    }
}

impl RawFeed<ModeEvent> for ModeFeed {
    fn next(&mut self) -> BoxFuture<'_, Result<ModeEvent, Error>> {
        Box::pin(async move {
            loop {
                if let Some(item) = self.pending.pop_front() {
                    return item;
                }
                if let Some(error) = &self.ended {
                    return Err(error.clone());
                }
                if !self.started {
                    self.start().await;
                    continue;
                }
                tokio::select! {
                    biased;
                    item = self.receiver.recv() => match item {
                        Some(item) => self.push(item),
                        None => self.ended = Some(Error::lost("the dispatch mode watch ended")),
                    },
                    outcome = self.watch.as_mut() => self.watch_ended(outcome).await,
                }
            }
        })
    }
}

impl Live {
    pub(super) async fn open_mode_feed(&self) -> Result<Box<dyn RawFeed<ModeEvent>>, Error> {
        let key_text = dispatch_mode_key(self.segments.runner_type.as_str());
        let key = KvKey::new(key_text.clone()).map_err(|e| Error::invalid(e.to_string()))?;
        let prefix = KvPrefix::new(key_text.clone()).map_err(|e| Error::invalid(e.to_string()))?;
        let reader = PublishedLanguageReader::<Value>::open(&self.wire.fabric)
            .await
            .map_err(bucket_error)?;
        let (sender, receiver) = mpsc::unbounded_channel();
        let consumer = PublishedLanguageConsumer::<Value, _, _>::open(
            &self.wire.fabric,
            vec![prefix],
            |_: &Value| true,
            ModeSink::new(key_text, sender),
        )
        .await
        .map_err(bucket_error)?;
        let consumer = Arc::new(consumer);
        let watch: Watching = Box::pin(async move { consumer.watch().await });
        Ok(Box::new(ModeFeed {
            fabric: self.wire.fabric.clone(),
            reader,
            key,
            receiver,
            watch,
            pending: VecDeque::new(),
            last: None,
            started: false,
            ended: None,
        }))
    }
}
