use std::future::Future;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::Connection;
use br_util_nats_fabric::{ConsumeErrorKind, FabricError};
use contract_threads::{CreateThread, ThreadsCommand};

use super::Live;
use super::answers::outcome_coords;
use crate::chunk_lane::Wire;
use crate::error::{Error, Failure};

fn bus_error(error: FabricError) -> Error {
    match error {
        FabricError::Consume {
            kind: ConsumeErrorKind::NoStream,
            detail,
        } => Error::InfraMissing(detail),
        other => Error::unavailable(other.to_string()),
    }
}

async fn within<T>(
    wait: Duration,
    what: &str,
    call: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::time::timeout(wait, call).await.map_err(|_| {
        Error::Unavailable(Failure::wait(format!("the check of {what} did not answer")))
    })?
}

impl Live {
    pub(crate) async fn build(
        connection: &Connection,
        check_wait: Duration,
    ) -> Result<Self, Error> {
        let wire = Wire::of(connection);
        let fabric = wire.fabric();
        let commands = CreateThread::coords();
        within(check_wait, "the command stream", async {
            fabric
                .await_commands(&[&commands])
                .await
                .map(drop)
                .map_err(bus_error)
        })
        .await?;
        let events = outcome_coords();
        let refs: Vec<_> = events.iter().collect();
        within(check_wait, "the event stream", async {
            fabric
                .await_events(&refs)
                .await
                .map(drop)
                .map_err(bus_error)
        })
        .await?;
        within(check_wait, "the chunk stream", wire.check_streaming()).await?;
        let actor: ServiceAccountId = connection.service_account();
        Ok(Self {
            connection: connection.clone(),
            wire,
            actor,
        })
    }
}
