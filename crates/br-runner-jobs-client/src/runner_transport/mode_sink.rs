use std::collections::BTreeSet;
use std::future::{Future, ready};
use std::pin::Pin;

use br_util_nats_fabric::{KvKey, ProjectionSink};
use contract_jobs::dispatch_mode::{DispatchMode as WireMode, RunnerTypeDispatchMode};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::error::Error;
use crate::model::{DispatchMode, ModeEvent};

pub(super) type ModeItem = Result<ModeEvent, Error>;

#[derive(Debug)]
pub(super) struct SinkClosed;

impl std::fmt::Display for SinkClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the mode feed was dropped")
    }
}

pub(super) struct ModeSink {
    key: String,
    sender: mpsc::UnboundedSender<ModeItem>,
}

impl ModeSink {
    pub(super) fn new(key: String, sender: mpsc::UnboundedSender<ModeItem>) -> Self {
        Self { key, sender }
    }

    fn send(&self, item: ModeItem) -> Result<(), SinkClosed> {
        self.sender.send(item).map_err(|_| SinkClosed)
    }
}

pub(super) fn read(value: &Value) -> ModeItem {
    serde_json::from_value::<RunnerTypeDispatchMode>(value.clone())
        .map(|entry| {
            ModeEvent::Mode(match entry.dispatch_mode {
                WireMode::Push => DispatchMode::Push,
                WireMode::Claim => DispatchMode::Claim,
            })
        })
        .map_err(|e| Error::unreadable_json("the dispatch mode entry", &e))
}

type Done<'a, T> = Pin<Box<dyn Future<Output = Result<T, SinkClosed>> + Send + 'a>>;

impl ProjectionSink<Value> for ModeSink {
    type Error = SinkClosed;

    fn project<'a, 'b, 'c, 'async_trait>(
        &'a self,
        key: &'b KvKey,
        value: &'c Value,
    ) -> Done<'async_trait, ()>
    where
        'a: 'async_trait,
        'b: 'async_trait,
        'c: 'async_trait,
        Self: 'async_trait,
    {
        let result = if key.as_str() == self.key {
            self.send(read(value))
        } else {
            Ok(())
        };
        Box::pin(ready(result))
    }

    fn retract<'a, 'b, 'async_trait>(&'a self, key: &'b KvKey) -> Done<'async_trait, ()>
    where
        'a: 'async_trait,
        'b: 'async_trait,
        Self: 'async_trait,
    {
        let result = if key.as_str() == self.key {
            self.send(Ok(ModeEvent::NoEntry))
        } else {
            Ok(())
        };
        Box::pin(ready(result))
    }

    fn known_keys<'a, 'async_trait>(&'a self) -> Done<'async_trait, BTreeSet<KvKey>>
    where
        'a: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(ready(Ok(BTreeSet::new())))
    }
}
