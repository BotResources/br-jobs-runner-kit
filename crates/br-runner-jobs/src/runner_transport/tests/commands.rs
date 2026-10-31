use std::time::Duration;

use br_util_nats_fabric::{IntegrationCommand, command_subject};
use contract_jobs::command::COMMAND_TYPE_CREATE;
use contract_jobs::command_v3::{
    CONTRACT_V3_SCHEMA_VERSION, CallMode as WireMode, CreateJobV3, Payload,
};
use contract_jobs::event_v2::{JobCreationRejectedV2, JobQueuedV2, RequesterRef};
use serde_json::json;
use uuid::Uuid;

use super::live;
use super::live_standin::{Responder, publish_event, service_passport};
use super::live_world::{WAIT, World};
use crate::backend::{Backend, ChildCommand, CreateChild};
use crate::error::{Cause, Error, Operation};
use crate::model::{CallMode, Content, PayloadForm};

pub(super) fn create_command(job_id: Uuid, run_id: Uuid) -> CreateChild {
    CreateChild {
        command_id: Uuid::now_v7(),
        job_id,
        runner_type: "writer".to_owned(),
        mode: CallMode::Background {
            caller_address: Uuid::now_v7(),
        },
        config: Some(json!({ "tone": "dry" })),
        payload: PayloadForm::Inline(Content {
            message: "hello".to_owned(),
            attachments: Vec::new(),
        }),
        passport: service_passport(),
        parent_job_id: Uuid::now_v7(),
        initial_job_id: Uuid::now_v7(),
        run_id,
        max_attempts: Some(2),
    }
}

pub(super) async fn commands_on(
    world: &World,
    coords: br_util_nats_fabric::CommandCoords,
) -> Responder {
    Responder::start(&world.client, command_subject(&coords), None).await
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_service_that_queues_when_creating_then_ok_and_the_command_is_the_wire_create() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut seen = commands_on(
        &world,
        contract_jobs::cmd_job_create_v3_coords().expect("coords"),
    )
    .await;
    let (job_id, run_id) = (Uuid::now_v7(), Uuid::now_v7());
    let command = create_command(job_id, run_id);
    let (command_id, parent, initial, address) = (
        command.command_id,
        command.parent_job_id,
        command.initial_job_id,
        match command.mode {
            CallMode::Background { caller_address } => caller_address,
            CallMode::Sync => unreachable!("the test builds a background call"),
        },
    );
    let service = async {
        let envelope: IntegrationCommand<CreateJobV3> =
            serde_json::from_slice(&seen.request().await).expect("a wire command");
        assert_eq!(envelope.command_id, command_id);
        assert_eq!(envelope.command_type, COMMAND_TYPE_CREATE);
        assert_eq!(envelope.version, CONTRACT_V3_SCHEMA_VERSION);
        assert_eq!(envelope.metadata.correlation_id, command_id);
        let wire = envelope.payload;
        assert_eq!(
            (
                wire.job_id,
                wire.runner_type.as_str(),
                wire.producer.as_str()
            ),
            (job_id, "writer", "analyst")
        );
        assert_eq!(wire.acting_run_id, Some(run_id));
        assert_eq!(wire.acting_instance_key.as_deref(), Some("pod-1"));
        assert_eq!(
            (wire.parent_job_id, wire.initial_job_id),
            (Some(parent), Some(initial))
        );
        assert_eq!(
            (wire.mode, wire.caller_address),
            (Some(WireMode::Background), Some(address))
        );
        assert_eq!(wire.max_attempts, Some(2));
        assert_eq!(wire.config, Some(json!({ "tone": "dry" })));
        assert!(matches!(wire.payload, Some(Payload::Inline { .. })));
        publish_event(
            &world.js,
            &contract_jobs::evt_job_queued_v2_coords().expect("coords"),
            command_id,
            JobQueuedV2 {
                job_id,
                parent_job_id: parent,
                initial_job_id: initial,
                runner_type: "writer".to_owned(),
                requester: RequesterRef::Run { run_id },
                restart_of_job_id: None,
                retry_of_job_id: None,
            },
        )
        .await;
    };
    let (result, ()) = tokio::join!(
        live.child_command(ChildCommand::Create(command), WAIT),
        service
    );
    result.map_err(|e| e.to_string()).expect("accepted");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_service_that_rejects_the_creation_when_creating_then_refused_with_its_reason_code()
{
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut seen = commands_on(
        &world,
        contract_jobs::cmd_job_create_v3_coords().expect("coords"),
    )
    .await;
    let command = create_command(Uuid::now_v7(), Uuid::now_v7());
    let (command_id, job_id) = (command.command_id, command.job_id);
    let service = async {
        seen.request().await;
        publish_event(
            &world.js,
            &contract_jobs::evt_job_creation_rejected_v2_coords().expect("coords"),
            command_id,
            JobCreationRejectedV2 {
                job_id,
                parent_job_id: Uuid::now_v7(),
                initial_job_id: Uuid::now_v7(),
                reason_code: "unknown_runner_type".to_owned(),
                message: "nobody runs writer".to_owned(),
                params: None,
            },
        )
        .await;
    };
    let (result, ()) = tokio::join!(
        live.child_command(ChildCommand::Create(command), WAIT),
        service
    );
    match result.expect_err("refused") {
        Error::Refused(refusal) => {
            assert_eq!(refusal.operation, Operation::Create);
            assert_eq!(refusal.reason, "unknown_runner_type");
            assert_eq!(refusal.message, "nobody runs writer");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_outcome_of_another_command_when_creating_then_it_is_not_taken_for_ours() {
    let world = World::ready().await;
    let live = super::live_with_wait(&world, "analyst", "pod-1", Duration::from_millis(600)).await;
    let mut seen = commands_on(
        &world,
        contract_jobs::cmd_job_create_v3_coords().expect("coords"),
    )
    .await;
    let command = create_command(Uuid::now_v7(), Uuid::now_v7());
    let service = async {
        seen.request().await;
        publish_event(
            &world.js,
            &contract_jobs::evt_job_creation_rejected_v2_coords().expect("coords"),
            Uuid::now_v7(),
            JobCreationRejectedV2 {
                job_id: Uuid::now_v7(),
                parent_job_id: Uuid::now_v7(),
                initial_job_id: Uuid::now_v7(),
                reason_code: "other".to_owned(),
                message: String::new(),
                params: None,
            },
        )
        .await;
    };
    let (result, ()) = tokio::join!(
        live.child_command(ChildCommand::Create(command), Duration::from_millis(600)),
        service
    );
    match result.expect_err("no outcome of ours") {
        Error::Lost(failure) => assert_eq!(failure.cause, Cause::WaitFired),
        other => panic!("{other:?}"),
    }
}
