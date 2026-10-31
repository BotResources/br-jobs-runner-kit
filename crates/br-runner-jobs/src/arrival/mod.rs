mod resolve;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use uuid::Uuid;

use crate::backend::RawFeed;
use crate::error::Error;
use crate::graphql;
use crate::model::{ItemArrived, SyncEnd, UnreadItem};
use crate::work::WorkCore;

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Arrival {
    Item(ItemArrived),
    SyncEnd {
        child_job_id: Uuid,
        end: SyncEnd,
    },
    Unreadable {
        child_job_id: Option<Uuid>,
        error: Error,
    },
}

pub struct ArrivalFeed {
    core: Arc<WorkCore>,
    raw: Box<dyn RawFeed<ItemArrived>>,
    buffer: VecDeque<Arrival>,
    resolving: Option<ItemArrived>,
    ended: Option<Error>,
}

impl ArrivalFeed {
    pub(crate) async fn open(core: Arc<WorkCore>) -> Result<Self, Error> {
        let raw = core.backend.arrival_feed(core.run_id).await?;
        Self::from_raw(core, raw).await
    }

    pub(crate) async fn from_raw(
        core: Arc<WorkCore>,
        raw: Box<dyn RawFeed<ItemArrived>>,
    ) -> Result<Self, Error> {
        let mut buffer = VecDeque::new();
        match graphql::unread(core.backend.as_ref(), &core.ids(), None).await {
            Ok(items) => replay(&items, &mut buffer),
            Err(error @ Error::Unreadable(_)) => buffer.push_back(Arrival::Unreadable {
                child_job_id: None,
                error,
            }),
            Err(error) => return Err(error),
        }
        Ok(Self {
            core,
            raw,
            buffer,
            resolving: None,
            ended: None,
        })
    }

    pub async fn next(&mut self) -> Result<Arrival, Error> {
        loop {
            if let Some(arrival) = self.buffer.pop_front() {
                return Ok(arrival);
            }
            if let Some(error) = &self.ended {
                return Err(error.clone());
            }
            if let Some(event) = self.resolving {
                self.resolve(event).await;
                continue;
            }
            match self.raw.next().await {
                Ok(event) => {
                    self.buffer.push_back(Arrival::Item(event));
                    if resolve::is_run_level_notice(&event) {
                        self.resolving = Some(event);
                    }
                }
                Err(error @ Error::Unreadable(_)) => {
                    return Ok(Arrival::Unreadable {
                        child_job_id: None,
                        error,
                    });
                }
                Err(error) => {
                    self.ended = Some(error.clone());
                    return Err(error);
                }
            }
        }
    }

    async fn resolve(&mut self, event: ItemArrived) {
        let read = graphql::unread(self.core.backend.as_ref(), &self.core.ids(), None).await;
        self.resolving = None;
        match read {
            Ok(items) => {
                let found = items.iter().find(|item| item.item_id() == event.item_id);
                if let Some(arrival) = found.and_then(resolve::sync_end) {
                    self.buffer.push_back(arrival);
                }
            }
            Err(error @ Error::Unreadable(_)) => self.buffer.push_back(Arrival::Unreadable {
                child_job_id: None,
                error,
            }),
            Err(error) => self.ended = Some(error),
        }
    }
}

fn replay(items: &[UnreadItem], buffer: &mut VecDeque<Arrival>) {
    for item in items {
        buffer.push_back(Arrival::Item(resolve::item_arrived(item)));
        if let Some(arrival) = resolve::sync_end(item) {
            buffer.push_back(arrival);
        }
    }
}

impl fmt::Debug for ArrivalFeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArrivalFeed")
            .field("ended", &self.ended.is_some())
            .finish_non_exhaustive()
    }
}
