use std::time::Duration;

use contract_threads::Chunk;
use uuid::Uuid;

use super::ThreadClient;
use crate::backend::{Command, Outcome};
use crate::error::Error;

impl ThreadClient {
    pub(crate) async fn open_numbered(
        &self,
        command: Command,
        wait: Duration,
    ) -> Result<Outcome, Error> {
        self.backend.command(command, wait).await
    }

    pub(crate) async fn send_numbered(
        &self,
        message_id: Uuid,
        seq: u64,
        chunk: Chunk,
        wait: Duration,
    ) -> Result<(), Error> {
        self.backend.chunk(message_id, seq, chunk, wait).await
    }

    pub(crate) async fn close_numbered(
        &self,
        command: Command,
        wait: Duration,
    ) -> Result<Outcome, Error> {
        self.backend.command(command, wait).await
    }
}
