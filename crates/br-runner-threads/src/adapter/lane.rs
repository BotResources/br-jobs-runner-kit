use std::sync::Arc;

use br_llm_messages::ResponseDelta;
use uuid::Uuid;

use super::inner::{Inner, Slot};
use crate::backend::BoxFuture;
use crate::error::Error;
use crate::response::{DeltaLane, OrderGuard};
use crate::stream::writes::delta_chunk;

pub(crate) struct QueueLane {
    inner: Arc<Inner>,
    slot: Arc<Slot>,
    message: Uuid,
}

impl QueueLane {
    pub(crate) fn new(inner: Arc<Inner>, slot: Arc<Slot>, message: Uuid) -> Self {
        Self {
            inner,
            slot,
            message,
        }
    }
}

impl DeltaLane for QueueLane {
    fn order<'a>(&'a self) -> BoxFuture<'a, Option<OrderGuard>> {
        Box::pin(async move { Some(self.slot.order_guard().await) })
    }

    fn send_delta<'a>(&'a self, delta: &'a ResponseDelta) -> BoxFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            let chunk = delta_chunk(self.message, delta)?;
            let mut mem = self.slot.mem.lock().await;
            self.inner
                .push_chunk(
                    &self.slot,
                    &mut mem,
                    self.message,
                    vec![delta.entry().as_uuid()],
                    chunk,
                    Uuid::now_v7(),
                )
                .await
        })
    }
}
