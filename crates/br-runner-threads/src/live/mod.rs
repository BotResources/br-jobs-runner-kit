mod answers;
mod build;
mod command;

use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::Connection;
use contract_threads::Chunk;
use serde_json::Value;
use uuid::Uuid;

use crate::backend::{Backend, BoxFuture, Command, Outcome};
use crate::chunk_lane::Wire;
use crate::error::Error;
use crate::from_connection;

pub(crate) struct Live {
    connection: Connection,
    wire: Wire,
    actor: ServiceAccountId,
}

impl Backend for Live {
    fn command<'a>(
        &'a self,
        command: Command,
        wait: Duration,
    ) -> BoxFuture<'a, Result<Outcome, Error>> {
        Box::pin(self.run_command(command, wait))
    }

    fn chunk<'a>(
        &'a self,
        message_id: Uuid,
        seq: u64,
        chunk: Chunk,
        wait: Duration,
    ) -> BoxFuture<'a, Result<(), Error>> {
        Box::pin(async move { self.wire.publish_chunk(message_id, seq, chunk, wait).await })
    }

    fn graphql<'a>(
        &'a self,
        document: &'a str,
        variables: Value,
    ) -> BoxFuture<'a, Result<Value, Error>> {
        Box::pin(async move {
            self.connection
                .graphql(document, variables)
                .await
                .map_err(from_connection::error)
        })
    }
}
