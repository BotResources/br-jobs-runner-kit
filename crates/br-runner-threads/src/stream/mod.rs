mod lane;
pub(crate) mod shared;
pub(crate) mod writes;

use std::sync::Arc;

use br_llm_messages::{EntryId, ResponseDelta, TurnEntry};
use uuid::Uuid;

use crate::annotation::Annotation;
use crate::error::Error;
use crate::response::{ResponseControl, ResponseSink, pair};

use lane::HandleLane;
use shared::Core;

#[derive(Clone)]
pub struct StreamWriter {
    core: Arc<Core>,
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

    pub fn response(&self, entry: EntryId) -> Result<(ResponseSink, ResponseControl), Error> {
        response_of(&self.core, entry)
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

    pub fn response(&self, entry: EntryId) -> Result<(ResponseSink, ResponseControl), Error> {
        response_of(&self.core, entry)
    }
}

fn response_of(core: &Arc<Core>, entry: EntryId) -> Result<(ResponseSink, ResponseControl), Error> {
    let turn =
        br_llm_messages::TurnId::from_uuid(core.message_id).map_err(|e| Error::library(&e))?;
    Ok(pair(
        turn,
        entry,
        Arc::new(HandleLane::new(Arc::clone(core))),
    ))
}
