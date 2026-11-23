mod store;

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};

use tokio::sync::watch;
use uuid::Uuid;

use crate::backend::BoxFuture;
use crate::error::Error;
use crate::model::SyncEnd;

pub(crate) use store::{EndStore, Outcome};

pub struct EndWait {
    inner: BoxFuture<'static, Result<SyncEnd, Error>>,
}

async fn watch_store(
    store: Weak<EndStore>,
    child_job_id: Uuid,
    generation: u64,
    mut changes: watch::Receiver<u64>,
) -> Outcome {
    loop {
        let Some(strong) = store.upgrade() else {
            return Err(Error::BookDropped);
        };
        let decided = strong.decide(child_job_id, generation);
        drop(strong);
        if let Some(outcome) = decided {
            return outcome;
        }
        if changes.changed().await.is_err() {
            return Err(Error::BookDropped);
        }
    }
}

impl EndWait {
    pub(crate) fn watching(store: &Arc<EndStore>, child_job_id: Uuid) -> Self {
        let opened = store.open();
        Self::from_future(watch_store(
            Arc::downgrade(store),
            child_job_id,
            opened.generation,
            opened.changes,
        ))
    }

    pub(crate) fn watching_weak(store: &Weak<EndStore>, child_job_id: Uuid) -> Self {
        match store.upgrade() {
            Some(strong) => Self::watching(&strong, child_job_id),
            None => Self::dropped(),
        }
    }

    pub(crate) fn dropped() -> Self {
        Self::from_future(std::future::ready(Err(Error::BookDropped)))
    }

    pub(crate) fn from_future(future: impl Future<Output = Outcome> + Send + 'static) -> Self {
        Self {
            inner: Box::pin(future),
        }
    }
}

impl Future for EndWait {
    type Output = Result<SyncEnd, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}

impl fmt::Debug for EndWait {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EndWait").finish_non_exhaustive()
    }
}
