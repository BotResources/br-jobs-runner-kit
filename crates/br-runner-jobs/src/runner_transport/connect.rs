use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::context::ContextBuilder;
use br_runner_connection::Connection;
use br_util_nats_fabric::{
    Fabric, INTEGRATION_CMD, INTEGRATION_EVT, KV_PUBLISHED_LANGUAGE, PublishedLanguageReader,
};
use contract_jobs::runner::{
    CANCEL_BUCKET, LOG_STREAM, PRESENCE_BUCKET, STATUS_STREAM, TRIGGER_STREAM,
};
use contract_jobs::segment::SubjectSegment;
use serde_json::Value;

use super::arrivals::Arrivals;
use super::{Live, Segments, Wire, fail};
use crate::error::{Error, Failure};
use crate::runner_type::{RunnerType, segment_error};

const UNBOUNDED_ACK_WAIT: Duration = Duration::from_secs(60 * 60 * 24 * 365);

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
    pub(crate) async fn connect(
        runner_type: RunnerType,
        instance_key: &str,
        reply_wait: Duration,
        connection: Connection,
    ) -> Result<Self, Error> {
        let instance_segment =
            SubjectSegment::instance_key(instance_key).map_err(|error| segment_error(&error))?;
        if reply_wait.is_zero() {
            return Err(Error::invalid("the reply wait is zero"));
        }
        let jetstream = ContextBuilder::new()
            .timeout(reply_wait)
            .ack_timeout(UNBOUNDED_ACK_WAIT)
            .build(connection.bus().clone());
        let fabric = Fabric::new(jetstream.clone());
        if !fabric.reachable() {
            return Err(Error::unavailable("the bus connection is not connected"));
        }
        let wire = Wire { jetstream, fabric };
        for stream in [TRIGGER_STREAM, STATUS_STREAM, LOG_STREAM] {
            let object = format!("the stream {stream}");
            within(reply_wait, &object, async {
                wire.jetstream
                    .get_stream(stream)
                    .await
                    .map(drop)
                    .map_err(|e| fail::unsent(&object, &e))
            })
            .await?;
        }
        let presence = bucket(&wire, reply_wait, PRESENCE_BUCKET).await?;
        let cancel = bucket(&wire, reply_wait, CANCEL_BUCKET).await?;
        if presence.stream.cached_info().config.max_age.is_zero() {
            return Err(Error::InfraMissing(format!(
                "the bucket {PRESENCE_BUCKET} has no time to live"
            )));
        }
        check_fabric(&wire.fabric, reply_wait).await?;
        let segments = Segments {
            runner_type: runner_type.segment().clone(),
            instance_key: instance_segment,
        };
        Ok(Self {
            runner_type,
            instance_key: instance_key.to_owned(),
            reply_wait,
            arrivals: Arc::new(Arrivals::new(&wire, &segments)?),
            segments,
            connection,
            wire,
            presence,
            cancel,
        })
    }
}

async fn bucket(
    wire: &Wire,
    wait: Duration,
    name: &str,
) -> Result<async_nats::jetstream::kv::Store, Error> {
    let object = format!("the bucket {name}");
    within(wait, &object, async {
        wire.jetstream
            .get_key_value(name)
            .await
            .map_err(|e| fail::unsent(&object, &e))
    })
    .await
}

async fn check_fabric(fabric: &Fabric, wait: Duration) -> Result<(), Error> {
    let object = format!("the bucket {KV_PUBLISHED_LANGUAGE}");
    within(wait, &object, async {
        PublishedLanguageReader::<Value>::open(fabric)
            .await
            .map(drop)
            .map_err(|_| fail::missing(&object))
    })
    .await?;
    let arrived = contract_jobs::evt_run_item_arrived_v1_coords()
        .map_err(|e| Error::invalid(e.to_string()))?;
    let events = format!("the stream {INTEGRATION_EVT}");
    within(wait, &events, async {
        fabric
            .await_events(&[&arrived])
            .await
            .map(drop)
            .map_err(|e| fail::fabric_unsent(&events, e))
    })
    .await?;
    let created =
        contract_jobs::cmd_job_create_v3_coords().map_err(|e| Error::invalid(e.to_string()))?;
    let commands = format!("the stream {INTEGRATION_CMD}");
    within(wait, &commands, async {
        fabric
            .await_commands(&[&created])
            .await
            .map(drop)
            .map_err(|e| fail::fabric_unsent(&commands, e))
    })
    .await
}
