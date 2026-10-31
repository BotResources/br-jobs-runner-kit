use std::sync::Arc;

use br_llm_messages::ResponseDelta;

use super::shared::Core;
use super::writes;
use crate::backend::BoxFuture;
use crate::error::Error;
use crate::response::{DeltaLane, OrderGuard};

pub(crate) struct HandleLane {
    core: Arc<Core>,
}

impl HandleLane {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }
}

impl DeltaLane for HandleLane {
    fn order<'a>(&'a self) -> BoxFuture<'a, Option<OrderGuard>> {
        Box::pin(async { None })
    }

    fn send_delta<'a>(&'a self, delta: &'a ResponseDelta) -> BoxFuture<'a, Result<(), Error>> {
        Box::pin(async move { writes::delta(&self.core, delta).await })
    }
}
