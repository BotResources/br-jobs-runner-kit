use std::sync::Mutex;

use br_agentic_contract::{PortFuture, ThreadRef};
use uuid::Uuid;

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredItem {
    pub thread: ThreadRef,
    pub id: Uuid,
    pub bytes: Vec<u8>,
}

pub trait QueueStore: Send + Sync {
    fn append<'a>(&'a self, item: StoredItem) -> PortFuture<'a, Result<(), Error>>;

    fn list<'a>(&'a self) -> PortFuture<'a, Result<Vec<StoredItem>, Error>>;

    fn remove<'a>(&'a self, thread: &'a ThreadRef, id: Uuid) -> PortFuture<'a, Result<(), Error>>;
}

#[derive(Default)]
pub struct MemoryQueueStore {
    items: Mutex<Vec<StoredItem>>,
}

impl MemoryQueueStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn items(&self) -> std::sync::MutexGuard<'_, Vec<StoredItem>> {
        self.items
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl QueueStore for MemoryQueueStore {
    fn append<'a>(&'a self, item: StoredItem) -> PortFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            self.items().push(item);
            Ok(())
        })
    }

    fn list<'a>(&'a self) -> PortFuture<'a, Result<Vec<StoredItem>, Error>> {
        Box::pin(async move { Ok(self.items().clone()) })
    }

    fn remove<'a>(&'a self, thread: &'a ThreadRef, id: Uuid) -> PortFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            self.items()
                .retain(|item| !(item.id == id && &item.thread == thread));
            Ok(())
        })
    }
}
