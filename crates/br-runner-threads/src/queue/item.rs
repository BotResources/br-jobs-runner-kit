use br_agentic_contract::ThreadRef;
use contract_threads::Chunk;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::store::StoredItem;
use crate::backend::CommandBody;
use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ItemKind {
    WholeMessage,
    Opening,
    Chunk,
    Closing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum Payload {
    Command(CommandBody),
    Chunk { seq: u64, chunk: Chunk },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Item {
    pub(crate) id: Uuid,
    pub(crate) thread: Uuid,
    pub(crate) agent: Uuid,
    pub(crate) message: Uuid,
    pub(crate) entries: Vec<Uuid>,
    pub(crate) kind: ItemKind,
    pub(crate) payload: Payload,
}

impl Item {
    pub(crate) fn stored(&self, thread: ThreadRef) -> Result<StoredItem, Error> {
        let bytes = serde_json::to_vec(self).map_err(|e| Error::Store(e.to_string()))?;
        Ok(StoredItem {
            thread,
            id: self.id,
            bytes,
        })
    }

    pub(crate) fn read(stored: &StoredItem) -> Result<Self, Error> {
        let item: Item = serde_json::from_slice(&stored.bytes)
            .map_err(|e| Error::Store(format!("a kept item cannot be read: {e}")))?;
        let thread = ThreadRef::from_uuid(item.thread).map_err(|_| {
            Error::Store("a kept item names a thread that is not a UUIDv7".to_owned())
        })?;
        if thread != stored.thread || item.id != stored.id {
            return Err(Error::Store(
                "a kept item does not match the thread or id it was kept under".to_owned(),
            ));
        }
        Ok(item)
    }
}
