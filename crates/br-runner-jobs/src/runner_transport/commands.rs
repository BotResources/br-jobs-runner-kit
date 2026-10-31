use std::time::Duration;

use br_core_kernel::Actor;
use br_util_nats_fabric::{
    CommandCoords, CorrelatedAwaiter, CorrelatedMatch, EventCoords, EventMetadata, INTEGRATION_CMD,
    INTEGRATION_EVT, IntegrationCommand, IntegrationEvent, event_subject,
};
use chrono::Utc;
use contract_jobs::command::{COMMAND_TYPE_CANCEL, COMMAND_TYPE_CREATE, COMMAND_TYPE_FINISH};
use contract_jobs::command_v3::{
    CONTRACT_V3_SCHEMA_VERSION, CallMode as WireMode, CancelJobV3, CreateJobV3, FinishJobV3,
    InlineAttachment, Payload,
};
use contract_jobs::event_v2::{JobCommandRejected, JobCreationRejectedV2};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::time::Instant;
use uuid::Uuid;

use super::{Live, fail};
use crate::backend::{ChildCommand, CreateChild};
use crate::error::{Error, Operation};
use crate::model::{CallMode, PayloadForm};

fn payload_of(form: PayloadForm) -> Payload {
    match form {
        PayloadForm::Inline(content) => Payload::Inline {
            message: content.message,
            attachments: content
                .attachments
                .into_iter()
                .map(|a| InlineAttachment {
                    name: a.name,
                    content_type: a.content_type,
                    content: a.content,
                })
                .collect(),
        },
        PayloadForm::Kept => Payload::Kept,
        PayloadForm::Reference {
            service,
            document_id,
        } => Payload::Reference {
            service,
            document_id,
        },
    }
}

fn coords<T>(made: Result<T, br_util_nats_fabric::CoordError>) -> Result<T, Error> {
    made.map_err(|e| Error::invalid(e.to_string()))
}

fn decode<T: DeserializeOwned>(found: &CorrelatedMatch) -> Result<T, Error> {
    serde_json::from_slice::<IntegrationEvent<T>>(&found.payload)
        .map(|event| event.payload)
        .map_err(|e| Error::Unreadable(format!("the outcome of a command: {e}")))
}

struct Outcomes {
    awaiter: CorrelatedAwaiter,
    success: EventCoords,
    rejection: Option<EventCoords>,
}

impl Live {
    fn envelope<T>(
        &self,
        command_id: Uuid,
        command_type: &str,
        payload: T,
    ) -> IntegrationCommand<T> {
        IntegrationCommand::new(
            command_id,
            command_type,
            CONTRACT_V3_SCHEMA_VERSION,
            Utc::now(),
            EventMetadata::new(
                Actor::Service(self.connection.service_account()),
                command_id,
            ),
            payload,
        )
    }

    pub(super) async fn child(&self, command: ChildCommand, wait: Duration) -> Result<(), Error> {
        match command {
            ChildCommand::Create(create) => self.create_child(create, wait).await,
            ChildCommand::Finish {
                command_id,
                job_id,
                run_id,
            } => {
                let payload = FinishJobV3 {
                    job_id,
                    acting_run_id: Some(run_id),
                    acting_instance_key: Some(self.instance_key.clone()),
                };
                let envelope = self.envelope(command_id, COMMAND_TYPE_FINISH, payload);
                let coords_of = coords(contract_jobs::cmd_job_finish_v3_coords())?;
                let success = coords(contract_jobs::evt_job_completed_v2_coords())?;
                let rejection = coords(contract_jobs::evt_job_command_rejected_v1_coords())?;
                self.run(
                    coords_of,
                    envelope,
                    success,
                    Some(rejection),
                    Operation::Finish,
                    wait,
                )
                .await
            }
            ChildCommand::Cancel {
                command_id,
                job_id,
                run_id,
            } => {
                let payload = CancelJobV3 {
                    job_id,
                    acting_run_id: Some(run_id),
                    acting_instance_key: Some(self.instance_key.clone()),
                };
                let envelope = self.envelope(command_id, COMMAND_TYPE_CANCEL, payload);
                let coords_of = coords(contract_jobs::cmd_job_cancel_v3_coords())?;
                let success = coords(contract_jobs::evt_job_cancel_accepted_v1_coords())?;
                let rejection = coords(contract_jobs::evt_job_command_rejected_v1_coords())?;
                self.run(
                    coords_of,
                    envelope,
                    success,
                    Some(rejection),
                    Operation::Cancel,
                    wait,
                )
                .await
            }
        }
    }

    async fn create_child(&self, create: CreateChild, wait: Duration) -> Result<(), Error> {
        let (mode, caller_address) = match create.mode {
            CallMode::Sync => (WireMode::Sync, None),
            CallMode::Background { caller_address } => (WireMode::Background, Some(caller_address)),
        };
        let payload = CreateJobV3 {
            job_id: create.job_id,
            runner_type: create.runner_type,
            producer: self.runner_type.clone(),
            config: create.config,
            payload: Some(payload_of(create.payload)),
            passport: Some(create.passport),
            triggered_by: None,
            parent_job_id: Some(create.parent_job_id),
            initial_job_id: Some(create.initial_job_id),
            mode: Some(mode),
            caller_address,
            acting_run_id: Some(create.run_id),
            acting_instance_key: Some(self.instance_key.clone()),
            source_bc: None,
            source_entity_id: None,
            max_attempts: create.max_attempts,
            priority: None,
        };
        let envelope = self.envelope(create.command_id, COMMAND_TYPE_CREATE, payload);
        let coords_of = coords(contract_jobs::cmd_job_create_v3_coords())?;
        let success = coords(contract_jobs::evt_job_queued_v2_coords())?;
        let rejection = coords(contract_jobs::evt_job_creation_rejected_v2_coords())?;
        self.run(
            coords_of,
            envelope,
            success,
            Some(rejection),
            Operation::Create,
            wait,
        )
        .await
    }

    async fn run<T: Serialize>(
        &self,
        command_coords: CommandCoords,
        envelope: IntegrationCommand<T>,
        success: EventCoords,
        rejection: Option<EventCoords>,
        operation: Operation,
        wait: Duration,
    ) -> Result<(), Error> {
        let deadline = Instant::now() + wait;
        let command_id = envelope.command_id;
        let mut watched: Vec<&EventCoords> = vec![&success];
        if let Some(rejection) = &rejection {
            watched.push(rejection);
        }
        let events = format!("the stream {INTEGRATION_EVT}");
        let awaiter = tokio::time::timeout_at(deadline, self.wire.fabric.await_events(&watched))
            .await
            .map_err(|_| Error::wait_fired("the outcome events could not be awaited in time"))?
            .map_err(|e| fail::fabric_unsent(&events, e))?;
        let commands = format!("the stream {INTEGRATION_CMD}");
        tokio::time::timeout_at(
            deadline,
            self.wire.fabric.publish_command_with_id(
                &command_coords,
                &envelope,
                &command_id.to_string(),
            ),
        )
        .await
        .map_err(|_| Error::wait_fired("the command was not confirmed by the bus"))?
        .map_err(|e| fail::fabric(&commands, e))?;
        let mut outcomes = Outcomes {
            awaiter,
            success,
            rejection,
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        outcomes.settle(command_id, remaining, operation).await
    }
}

impl Outcomes {
    async fn settle(
        &mut self,
        correlation_id: Uuid,
        wait: Duration,
        operation: Operation,
    ) -> Result<(), Error> {
        let found = self
            .awaiter
            .await_correlation(correlation_id, wait)
            .await
            .map_err(|e| fail::fabric("the outcome events", e))?
            .ok_or_else(|| Error::wait_fired("no outcome event came within the wait"))?;
        if found.subject == event_subject(&self.success) {
            return Ok(());
        }
        let rejected = match (&self.rejection, operation) {
            (Some(rejection), Operation::Create) if found.subject == event_subject(rejection) => {
                let rejected: JobCreationRejectedV2 = decode(&found)?;
                (rejected.reason_code, rejected.message)
            }
            (Some(rejection), _) if found.subject == event_subject(rejection) => {
                let rejected: JobCommandRejected = decode(&found)?;
                (rejected.reason_code, rejected.message)
            }
            _ => {
                return Err(Error::Unreadable(format!(
                    "an outcome event on an unexpected subject: {}",
                    found.subject
                )));
            }
        };
        Err(Error::refused(operation, rejected.0, rejected.1))
    }
}
