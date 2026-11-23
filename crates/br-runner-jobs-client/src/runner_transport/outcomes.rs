use std::time::Duration;

use br_util_nats_fabric::{
    CommandCoords, CorrelatedAwaiter, CorrelatedMatch, EventCoords, Fabric, INTEGRATION_CMD,
    INTEGRATION_EVT, IntegrationCommand, IntegrationEvent, event_subject,
};
use contract_jobs::event_v2::{JobCommandRejected, JobCreationRejectedV2};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::time::{Instant, timeout_at};

use super::fail;
use crate::backend::{BoxFuture, ChildSession};
use crate::error::{Error, Operation};

const LOOK_BEFORE_RESEND: Duration = Duration::from_millis(20);

fn look_before_resend(deadline: Instant) -> Duration {
    LOOK_BEFORE_RESEND.min(deadline.saturating_duration_since(Instant::now()) / 4)
}

pub(super) struct Outcomes {
    pub success: EventCoords,
    pub rejection: EventCoords,
    pub operation: Operation,
}

pub(super) struct Failing(pub Error);

impl ChildSession for Failing {
    fn attempt(&mut self, _wait: Duration) -> BoxFuture<'_, Result<(), Error>> {
        let error = self.0.clone();
        Box::pin(async move { Err(error) })
    }
}

pub(super) struct Session<T> {
    fabric: Fabric,
    command_coords: CommandCoords,
    envelope: IntegrationCommand<T>,
    outcomes: Outcomes,
    awaiter: Option<CorrelatedAwaiter>,
    published: bool,
}

enum Heard {
    Silence,
    Ended(Result<(), Error>),
}

fn decode<T: DeserializeOwned>(found: &CorrelatedMatch) -> Result<T, Error> {
    serde_json::from_slice::<IntegrationEvent<T>>(&found.payload)
        .map(|event| event.payload)
        .map_err(|e| Error::unreadable_json("the outcome of a command", &e))
}

impl<T: Serialize + Send + Sync> Session<T> {
    pub(super) fn new(
        fabric: Fabric,
        command_coords: CommandCoords,
        envelope: IntegrationCommand<T>,
        outcomes: Outcomes,
    ) -> Self {
        Self {
            fabric,
            command_coords,
            envelope,
            outcomes,
            awaiter: None,
            published: false,
        }
    }

    async fn subscribe(&mut self, deadline: Instant) -> Result<(), Error> {
        if self.awaiter.is_some() {
            return Ok(());
        }
        let events = format!("the stream {INTEGRATION_EVT}");
        let watched = [&self.outcomes.success, &self.outcomes.rejection];
        let awaiter = timeout_at(deadline, self.fabric.await_events(&watched))
            .await
            .map_err(|_| Error::wait_fired("the outcome events could not be awaited in time"))?
            .map_err(|e| fail::fabric_unsent(&events, e))?;
        self.awaiter = Some(awaiter);
        Ok(())
    }

    async fn hear(&mut self, wait: Duration) -> Heard {
        let command_id = self.envelope.command_id;
        let Some(awaiter) = self.awaiter.as_mut() else {
            return Heard::Silence;
        };
        let found = match awaiter.await_correlation(command_id, wait).await {
            Ok(Some(found)) => found,
            Ok(None) => return Heard::Silence,
            Err(e) => {
                self.awaiter = None;
                return Heard::Ended(Err(fail::fabric("the outcome events", e)));
            }
        };
        Heard::Ended(self.outcome_of(&found))
    }

    fn outcome_of(&self, found: &CorrelatedMatch) -> Result<(), Error> {
        let Outcomes {
            success,
            rejection,
            operation,
        } = &self.outcomes;
        if found.subject == event_subject(success) {
            return Ok(());
        }
        if found.subject != event_subject(rejection) {
            return Err(Error::Unreadable(format!(
                "an outcome event on an unexpected subject: {}",
                found.subject
            )));
        }
        let (reason, message) = match operation {
            Operation::Create => {
                let rejected: JobCreationRejectedV2 = decode(found)?;
                (rejected.reason_code, rejected.message)
            }
            _ => {
                let rejected: JobCommandRejected = decode(found)?;
                (rejected.reason_code, rejected.message)
            }
        };
        Err(Error::refused(*operation, reason, message))
    }

    async fn run(&mut self, wait: Duration) -> Result<(), Error> {
        let deadline = Instant::now() + wait;
        self.subscribe(deadline).await?;
        if self.published
            && let Heard::Ended(result) = self.hear(look_before_resend(deadline)).await
        {
            return result;
        }
        self.published = true;
        let commands = format!("the stream {INTEGRATION_CMD}");
        let command_id = self.envelope.command_id.to_string();
        timeout_at(
            deadline,
            self.fabric
                .publish_command_with_id(&self.command_coords, &self.envelope, &command_id),
        )
        .await
        .map_err(|_| Error::wait_fired("the command was not confirmed by the bus"))?
        .map_err(|e| fail::fabric(&commands, e))?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        match self.hear(remaining).await {
            Heard::Ended(result) => result,
            Heard::Silence => Err(Error::wait_fired("no outcome event came within the wait")),
        }
    }
}

impl<T: Serialize + Send + Sync> ChildSession for Session<T> {
    fn attempt(&mut self, wait: Duration) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(self.run(wait))
    }
}
