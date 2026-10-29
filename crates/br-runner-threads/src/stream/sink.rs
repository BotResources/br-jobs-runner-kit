use std::sync::Arc;

use br_llm_messages::ResponseDelta;

use crate::error::Error;
use crate::stream::shared::Core;
use crate::stream::writes;

#[derive(Clone)]
pub struct SinkFactory {
    core: Arc<Core>,
}

impl SinkFactory {
    pub(crate) fn new(core: Arc<Core>) -> Self {
        Self { core }
    }

    pub fn sink(&self) -> ResponseSink {
        ResponseSink {
            core: Arc::clone(&self.core),
        }
    }
}

pub struct ResponseSink {
    core: Arc<Core>,
}

impl ResponseSink {
    pub async fn send(&mut self, delta: ResponseDelta) -> Result<(), Error> {
        writes::delta(&self.core, &delta).await
    }
}
