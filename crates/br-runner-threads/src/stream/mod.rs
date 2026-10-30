pub(crate) mod shared;
mod sink;
mod writes;

use std::sync::Arc;

use br_llm_messages::{EntryId, ResponseDelta, TurnEntry};
use uuid::Uuid;

use crate::annotation::Annotation;
use crate::error::Error;

pub use sink::{ResponseSink, SinkFactory};

use shared::Core;

#[derive(Clone)]
pub struct StreamWriter {
    pub(crate) core: Arc<Core>,
}

impl StreamWriter {
    pub async fn entry(&self, entry: &TurnEntry) -> Result<(), Error> {
        writes::entry(&self.core, entry).await
    }

    pub async fn annotation(&self, after: EntryId, annotation: &Annotation) -> Result<(), Error> {
        writes::annotation(&self.core, after, annotation).await
    }

    pub async fn keepalive(&self) -> Result<(), Error> {
        writes::keepalive(&self.core).await
    }
}

pub struct StreamHandle {
    pub(crate) core: Arc<Core>,
}

impl StreamHandle {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }

    pub fn message_id(&self) -> Uuid {
        self.core.message_id
    }

    pub async fn delta(&self, delta: &ResponseDelta) -> Result<(), Error> {
        writes::delta(&self.core, delta).await
    }

    pub async fn entry(&self, entry: &TurnEntry) -> Result<(), Error> {
        writes::entry(&self.core, entry).await
    }

    pub async fn annotation(&self, after: EntryId, annotation: &Annotation) -> Result<(), Error> {
        writes::annotation(&self.core, after, annotation).await
    }

    pub async fn keepalive(&self) -> Result<(), Error> {
        writes::keepalive(&self.core).await
    }

    pub fn writer(&self) -> StreamWriter {
        StreamWriter {
            core: Arc::clone(&self.core),
        }
    }

    pub fn sink_factory(&self) -> SinkFactory {
        SinkFactory::new(Arc::clone(&self.core))
    }
}
