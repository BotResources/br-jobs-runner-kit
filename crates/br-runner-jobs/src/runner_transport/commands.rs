use br_core_kernel::Actor;
use br_util_nats_fabric::{EventMetadata, IntegrationCommand};
use chrono::Utc;
use contract_jobs::command::{COMMAND_TYPE_CANCEL, COMMAND_TYPE_CREATE, COMMAND_TYPE_FINISH};
use contract_jobs::command_v3::{
    CONTRACT_V3_SCHEMA_VERSION, CallMode as WireMode, CancelJobV3, CreateJobV3, FinishJobV3,
    InlineAttachment, Payload,
};
use serde::Serialize;
use uuid::Uuid;

use super::Live;
use super::outcomes::{Failing, Outcomes, Session};
use crate::backend::{ChildCommand, ChildSession, CreateChild};
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

fn outcome_coords(
    success: Result<br_util_nats_fabric::EventCoords, br_util_nats_fabric::CoordError>,
    rejection: Result<br_util_nats_fabric::EventCoords, br_util_nats_fabric::CoordError>,
    operation: Operation,
) -> Result<Outcomes, Error> {
    Ok(Outcomes {
        success: coords(success)?,
        rejection: coords(rejection)?,
        operation,
    })
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

    fn session<T: Serialize + Send + Sync + 'static>(
        &self,
        command_coords: Result<br_util_nats_fabric::CommandCoords, br_util_nats_fabric::CoordError>,
        envelope: IntegrationCommand<T>,
        outcomes: Result<Outcomes, Error>,
    ) -> Box<dyn ChildSession + '_> {
        let ready = coords(command_coords)
            .and_then(|command_coords| outcomes.map(|outcomes| (command_coords, outcomes)));
        match ready {
            Ok((command_coords, outcomes)) => Box::new(Session::new(
                self.wire.fabric.clone(),
                command_coords,
                envelope,
                outcomes,
            )),
            Err(error) => Box::new(Failing(error)),
        }
    }

    pub(super) fn child(&self, command: ChildCommand) -> Box<dyn ChildSession + '_> {
        match command {
            ChildCommand::Create(create) => self.create_child(create),
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
                self.session(
                    contract_jobs::cmd_job_finish_v3_coords(),
                    envelope,
                    outcome_coords(
                        contract_jobs::evt_job_completed_v2_coords(),
                        contract_jobs::evt_job_command_rejected_v1_coords(),
                        Operation::Finish,
                    ),
                )
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
                self.session(
                    contract_jobs::cmd_job_cancel_v3_coords(),
                    envelope,
                    outcome_coords(
                        contract_jobs::evt_job_cancel_accepted_v1_coords(),
                        contract_jobs::evt_job_command_rejected_v1_coords(),
                        Operation::Cancel,
                    ),
                )
            }
        }
    }

    fn create_child(&self, create: CreateChild) -> Box<dyn ChildSession + '_> {
        let (mode, caller_address) = match create.mode {
            CallMode::Sync => (WireMode::Sync, None),
            CallMode::Background { caller_address } => (WireMode::Background, Some(caller_address)),
        };
        let payload = CreateJobV3 {
            job_id: create.job_id,
            runner_type: create.runner_type.as_str().to_owned(),
            producer: self.runner_type.as_str().to_owned(),
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
        self.session(
            contract_jobs::cmd_job_create_v3_coords(),
            envelope,
            outcome_coords(
                contract_jobs::evt_job_queued_v2_coords(),
                contract_jobs::evt_job_creation_rejected_v2_coords(),
                Operation::Create,
            ),
        )
    }
}
