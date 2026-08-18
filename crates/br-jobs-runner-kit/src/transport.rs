use std::time::Duration;

use async_nats::jetstream::{self, Context};
use contract_jobs::runner::{
    CANCEL_BUCKET, LOG_STREAM, PRESENCE_BUCKET, STATUS_STREAM, TRIGGER_STREAM,
};
use serde::Serialize;

use crate::config::RunnerConfig;
use crate::error::HarnessError;

pub(crate) mod cancel;
pub(crate) mod presence;
pub(crate) mod publish;
pub(crate) mod triggers;

pub(crate) struct Transport {
    pub js: Context,
    pub presence_store: jetstream::kv::Store,
    pub cancel_store: jetstream::kv::Store,
    pub presence_ttl: Duration,
}

impl Transport {
    pub(crate) async fn bind(config: &RunnerConfig) -> Result<Self, HarnessError> {
        let client = async_nats::ConnectOptions::new()
            .retry_on_initial_connect()
            .connect(&config.nats_url)
            .await
            .map_err(|source| HarnessError::Connect {
                url: config.nats_url.clone(),
                source,
            })?;
        let js = jetstream::new(client);

        for stream in [TRIGGER_STREAM, STATUS_STREAM, LOG_STREAM] {
            js.get_stream(stream).await.map_err(|error| {
                HarnessError::infra(
                    "binding a declared runner-transport stream",
                    format!("{stream}: {error}"),
                )
            })?;
        }

        let cancel_store = js
            .get_key_value(CANCEL_BUCKET)
            .await
            .map_err(|error| HarnessError::infra("binding the declared cancel bucket", error))?;
        let presence_store = js
            .get_key_value(PRESENCE_BUCKET)
            .await
            .map_err(|error| HarnessError::infra("binding the declared presence bucket", error))?;

        let presence_stream = js
            .get_stream(format!("KV_{PRESENCE_BUCKET}"))
            .await
            .map_err(|error| HarnessError::infra("reading the presence bucket config", error))?;
        let presence_ttl = presence_stream.cached_info().config.max_age;
        if presence_ttl.is_zero() {
            return Err(HarnessError::PresenceBucketWithoutTtl);
        }

        Ok(Self {
            js,
            presence_store,
            cancel_store,
            presence_ttl,
        })
    }
}

pub(crate) async fn publish_acked<T: Serialize>(
    js: &Context,
    subject: String,
    payload: &T,
) -> Result<(), HarnessError> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| HarnessError::infra("serializing a runner-transport payload", error))?;
    js.publish(subject, bytes.into())
        .await
        .map_err(|error| HarnessError::infra("publishing a runner-transport fact", error))?
        .await
        .map_err(|error| HarnessError::infra("awaiting the broker ack of a fact", error))?;
    Ok(())
}
