use std::time::Duration;

use br_util_nats_fabric::IntegrationCommand;
use contract_jobs::command::{COMMAND_TYPE_CANCEL, COMMAND_TYPE_FINISH};
use contract_jobs::command_v3::{CancelJobV3, FinishJobV3};
use contract_jobs::event_v2::{JobCancelAccepted, JobCommandRejected, JobCompletedV2};
use serde_json::json;
use uuid::Uuid;

use super::commands::commands_on;
use super::live;
use super::live_standin::publish_event;
use super::live_world::{WAIT, World};
use crate::backend::{Backend, ChildCommand};
use crate::error::{Cause, Error, Operation};

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_no_outcome_event_when_finishing_then_lost_with_a_fired_wait() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let error = live
        .child_command(
            ChildCommand::Finish {
                command_id: Uuid::now_v7(),
                job_id: Uuid::now_v7(),
                run_id: Uuid::now_v7(),
            },
            Duration::from_millis(400),
        )
        .await
        .expect_err("silence");
    match error {
        Error::Lost(failure) => assert_eq!(failure.cause, Cause::WaitFired),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_service_that_completes_when_finishing_then_ok_and_the_command_is_the_wire_finish()
{
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut seen = commands_on(
        &world,
        contract_jobs::cmd_job_finish_v3_coords().expect("coords"),
    )
    .await;
    let (command_id, job_id, run_id) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let service = async {
        let envelope: IntegrationCommand<FinishJobV3> =
            serde_json::from_slice(&seen.request().await).expect("a wire command");
        assert_eq!(envelope.command_type, COMMAND_TYPE_FINISH);
        assert_eq!(envelope.command_id, command_id);
        assert_eq!(envelope.payload.job_id, job_id);
        assert_eq!(envelope.payload.acting_run_id, Some(run_id));
        assert_eq!(
            envelope.payload.acting_instance_key.as_deref(),
            Some("pod-1")
        );
        publish_event(
            &world.js,
            &contract_jobs::evt_job_completed_v2_coords().expect("coords"),
            command_id,
            JobCompletedV2 {
                job_id,
                parent_job_id: job_id,
                initial_job_id: job_id,
            },
        )
        .await;
    };
    let (result, ()) = tokio::join!(
        live.child_command(
            ChildCommand::Finish {
                command_id,
                job_id,
                run_id
            },
            WAIT
        ),
        service
    );
    result.map_err(|e| e.to_string()).expect("accepted");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_service_that_accepts_the_cancel_when_cancelling_then_ok_and_the_command_is_the_wire_cancel()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut seen = commands_on(
        &world,
        contract_jobs::cmd_job_cancel_v3_coords().expect("coords"),
    )
    .await;
    let (command_id, job_id, run_id) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let service = async {
        let envelope: IntegrationCommand<CancelJobV3> =
            serde_json::from_slice(&seen.request().await).expect("a wire command");
        assert_eq!(envelope.command_type, COMMAND_TYPE_CANCEL);
        assert_eq!(envelope.payload.job_id, job_id);
        assert_eq!(envelope.payload.acting_run_id, Some(run_id));
        publish_event(
            &world.js,
            &contract_jobs::evt_job_cancel_accepted_v1_coords().expect("coords"),
            command_id,
            JobCancelAccepted {
                job_id,
                parent_job_id: job_id,
                initial_job_id: job_id,
            },
        )
        .await;
    };
    let (result, ()) = tokio::join!(
        live.child_command(
            ChildCommand::Cancel {
                command_id,
                job_id,
                run_id
            },
            WAIT
        ),
        service
    );
    result.map_err(|e| e.to_string()).expect("accepted");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_service_that_rejects_the_command_when_cancelling_then_refused_with_the_cancel_operation()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut seen = commands_on(
        &world,
        contract_jobs::cmd_job_cancel_v3_coords().expect("coords"),
    )
    .await;
    let (command_id, job_id) = (Uuid::now_v7(), Uuid::now_v7());
    let service = async {
        seen.request().await;
        publish_event(
            &world.js,
            &contract_jobs::evt_job_command_rejected_v1_coords().expect("coords"),
            command_id,
            JobCommandRejected {
                command_type: COMMAND_TYPE_CANCEL.to_owned(),
                job_id,
                parent_job_id: None,
                initial_job_id: None,
                reason_code: "not_your_child".to_owned(),
                message: "the job is not yours".to_owned(),
                params: None,
            },
        )
        .await;
    };
    let (result, ()) = tokio::join!(
        live.child_command(
            ChildCommand::Cancel {
                command_id,
                job_id,
                run_id: Uuid::now_v7()
            },
            WAIT
        ),
        service
    );
    match result.expect_err("refused") {
        Error::Refused(refusal) => {
            assert_eq!(refusal.operation, Operation::Cancel);
            assert_eq!(refusal.reason, "not_your_child");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_outcome_payload_that_is_not_the_wire_shape_when_rejected_then_unreadable() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut seen = commands_on(
        &world,
        contract_jobs::cmd_job_finish_v3_coords().expect("coords"),
    )
    .await;
    let command_id = Uuid::now_v7();
    let service = async {
        seen.request().await;
        publish_event(
            &world.js,
            &contract_jobs::evt_job_command_rejected_v1_coords().expect("coords"),
            command_id,
            json!({ "nonsense": true }),
        )
        .await;
    };
    let finish = ChildCommand::Finish {
        command_id,
        job_id: Uuid::now_v7(),
        run_id: Uuid::now_v7(),
    };
    let (result, ()) = tokio::join!(live.child_command(finish, WAIT), service);
    assert!(matches!(
        result.expect_err("unreadable"),
        Error::Unreadable(_)
    ));
}
