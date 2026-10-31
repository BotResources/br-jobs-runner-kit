mod close;
mod graphql;
mod numbered;

use std::sync::Arc;
use std::time::Duration;

use br_runner_connection::Connection;

use crate::backend::Backend;
use crate::error::Error;
use crate::ids::require_wait;
use crate::live::Live;
use crate::message::{Confirmed, OpenStream, WholeMessage};
use crate::stream::StreamHandle;
use crate::stream::shared::Core;
use crate::thread::NewThread;
use crate::wire;

#[derive(Clone)]
pub struct ThreadClient {
    pub(crate) backend: Arc<dyn Backend>,
}

impl ThreadClient {
    pub async fn new(connection: &Connection, check_wait: Duration) -> Result<Self, Error> {
        require_wait(check_wait, "the check wait")?;
        let live = Live::build(connection, check_wait).await?;
        Ok(Self::over(Arc::new(live)))
    }

    pub(crate) fn over(backend: Arc<dyn Backend>) -> Self {
        Self { backend }
    }

    pub async fn create_thread(&self, thread: &NewThread) -> Result<Confirmed, Error> {
        require_wait(thread.wait, "the outcome wait")?;
        let command = wire::create_thread(thread)?;
        self.confirm(command, thread.wait).await
    }

    pub async fn write_message(&self, message: &WholeMessage) -> Result<Confirmed, Error> {
        require_wait(message.wait, "the outcome wait")?;
        let command = wire::whole_message(message)?;
        self.confirm(command, message.wait).await
    }

    pub async fn open_stream(&self, open: &OpenStream) -> Result<StreamHandle, Error> {
        require_wait(open.wait, "the outcome wait")?;
        let command = wire::open_message(open)?;
        self.confirm(command, open.wait).await?;
        Ok(StreamHandle::new(Core::new(
            Arc::clone(&self.backend),
            open.message_id.as_uuid(),
            open.wait,
        )))
    }

    async fn confirm(
        &self,
        command: crate::backend::Command,
        wait: Duration,
    ) -> Result<Confirmed, Error> {
        let outcome = self.backend.command(command, wait).await?;
        Ok(Confirmed {
            command_id: outcome.command_id,
            status: outcome.status,
        })
    }
}
