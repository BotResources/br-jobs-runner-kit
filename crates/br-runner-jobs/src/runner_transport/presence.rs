use std::time::Duration;

use contract_jobs::runner::{Capacity, Presence, RunnerStatus, WIRE_VERSION};
use contract_jobs::runner_transport::runner_presence_key;

use super::{Live, fail};
use crate::error::{Error, Failure};
use crate::model::PresenceStatus;

impl Live {
    fn presence_key(&self) -> String {
        runner_presence_key(&self.segments.runner_type, &self.segments.instance_key)
    }

    pub(super) async fn put_presence(
        &self,
        runner_version: String,
        status: PresenceStatus,
        capacity: u32,
    ) -> Result<(), Error> {
        let capacity = Capacity::try_from(capacity).map_err(Error::invalid)?;
        let presence = Presence {
            version: WIRE_VERSION,
            runner_type: self.runner_type.as_str().to_owned(),
            instance_key: self.instance_key.clone(),
            runner_version,
            status: match status {
                PresenceStatus::Ready => RunnerStatus::Ready,
                PresenceStatus::Draining => RunnerStatus::Draining,
            },
            capacity,
        };
        let bytes = serde_json::to_vec(&presence).map_err(|e| Error::invalid(e.to_string()))?;
        let object = "the presence bucket";
        tokio::time::timeout(
            self.reply_wait,
            self.presence.put(self.presence_key(), bytes.into()),
        )
        .await
        .map_err(|_| Error::wait_fired("the presence bucket did not accept the presence"))?
        .map(drop)
        .map_err(|e| fail::sent(object, &e))
    }

    pub(super) async fn delete_presence(&self) -> Result<(), Error> {
        let object = "the presence bucket";
        tokio::time::timeout(self.reply_wait, self.presence.delete(self.presence_key()))
            .await
            .map_err(|_| Error::wait_fired("the presence bucket did not accept the delete"))?
            .map_err(|e| fail::sent(object, &e))
    }

    pub(super) async fn time_to_live(&self) -> Result<Duration, Error> {
        let object = "the presence bucket";
        let mut stream = self.presence.stream.clone();
        let ttl = tokio::time::timeout(self.reply_wait, stream.info())
            .await
            .map_err(|_| Error::Unavailable(Failure::wait("the presence bucket did not answer")))?
            .map_err(|e| fail::unsent(object, &e))?
            .config
            .max_age;
        if ttl.is_zero() {
            return Err(Error::InfraMissing(
                "the presence bucket has no time to live".to_owned(),
            ));
        }
        Ok(ttl)
    }
}
