use std::future::Future;
use std::sync::Arc;

use crate::adapter::inner::{Inner, Slot};
use crate::detach::detach;
use crate::error::Error;
use crate::response::OrderGuard;

impl Slot {
    pub(crate) async fn order_guard(&self) -> OrderGuard {
        Arc::clone(&self.order).lock_owned().await
    }
}

impl Inner {
    pub(crate) async fn run_ordered<T, F, Fut>(
        self: &Arc<Self>,
        slot: &Arc<Slot>,
        order: OrderGuard,
        work: F,
    ) -> Result<T, Error>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Inner>, Arc<Slot>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, Error>> + Send + 'static,
    {
        let (inner, slot) = (Arc::clone(self), Arc::clone(slot));
        detach(async move {
            let _order = order;
            work(inner, slot).await
        })
        .await
        .unwrap_or_else(|| Err(Error::Store("the runtime stopped the call".to_owned())))
    }

    pub(crate) async fn ordered<T, F, Fut>(
        self: &Arc<Self>,
        slot: &Arc<Slot>,
        work: F,
    ) -> Result<T, Error>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Inner>, Arc<Slot>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, Error>> + Send + 'static,
    {
        let order = slot.order_guard().await;
        self.run_ordered(slot, order, work).await
    }
}
