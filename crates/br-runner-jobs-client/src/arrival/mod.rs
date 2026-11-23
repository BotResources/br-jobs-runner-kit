mod resolve;
mod settings;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use uuid::Uuid;

use crate::backend::RawFeed;
use crate::error::Error;
use crate::graphql::{self, Read};
use crate::model::{ItemArrived, SyncEnd};
use crate::work::WorkCore;

pub use settings::ArrivalSettings;
#[cfg(test)]
pub(crate) use settings::settings_of_tests;

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
    pub(crate) async fn open(
        core: Arc<WorkCore>,
        settings: ArrivalSettings,
    ) -> Result<Self, Error> {
        settings.check()?;
        let raw = core.backend.arrival_feed(core.run_id, settings).await?;
        Self::from_raw(core, raw).await
    }

    pub(crate) async fn from_raw(
        core: Arc<WorkCore>,
        raw: Box<dyn RawFeed<ItemArrived>>,
    ) -> Result<Self, Error> {
        let mut buffer = VecDeque::new();
        match graphql::unread_each(core.backend.as_ref(), &core.ids(), None).await {
            Ok(reads) => replay(&reads, &mut buffer),
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
        let read = graphql::unread_each(self.core.backend.as_ref(), &self.core.ids(), None).await;
        self.resolving = None;
        match read {
            Ok(reads) => {
                self.buffer.extend(
                    reads
                        .iter()
                        .filter_map(|read| resolve::resolved(read, &event)),
                );
            }
            Err(error @ Error::Unreadable(_)) => self.buffer.push_back(Arrival::Unreadable {
                child_job_id: None,
                error,
            }),
            Err(error) => self.ended = Some(error),
        }
    }
}

fn replay(reads: &[Read], buffer: &mut VecDeque<Arrival>) {
    for read in reads {
        match read {
            Read::Item(item) => {
                buffer.push_back(Arrival::Item(resolve::item_arrived(item)));
                if let Some(arrival) = resolve::sync_end(item) {
                    buffer.push_back(arrival);
                }
            }
            Read::Bad(bad) => buffer.push_back(resolve::bad(bad)),
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
