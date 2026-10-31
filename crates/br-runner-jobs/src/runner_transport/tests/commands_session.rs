use std::time::Duration;

use contract_jobs::event_v2::{JobCommandRejected, JobCompletedV2, JobQueuedV2, RequesterRef};
use uuid::Uuid;

use super::commands::{commands_on, create_command};
use super::live;
use super::live_standin::publish_event;
use super::live_world::{WAIT, World};
use crate::backend::{Backend, ChildCommand, ChildSession};
use crate::error::{Cause, Error, Operation};

const SHORT: Duration = Duration::from_millis(400);

fn queued(job_id: Uuid) -> JobQueuedV2 {
    JobQueuedV2 {
        job_id,
        parent_job_id: Uuid::now_v7(),
        initial_job_id: Uuid::now_v7(),
        runner_type: "writer".to_owned(),
        requester: RequesterRef::Run {
            run_id: Uuid::now_v7(),
        },
        restart_of_job_id: None,
        retry_of_job_id: None,
    }
}

fn silence(result: Result<(), Error>) {
    match result.expect_err("silence") {
        Error::Lost(failure) => assert_eq!(failure.cause, Cause::WaitFired),
        other => panic!("{other:?}"),
    }
}

async fn attempt(session: &mut dyn ChildSession, wait: Duration) -> Result<(), Error> {
    tokio::time::timeout(Duration::from_secs(10), session.attempt(wait))
        .await
        .expect("the attempt ended")
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_create_whose_outcome_comes_between_two_attempts_then_the_next_attempt_sees_it_and_sends_nothing()
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
    let mut session = live.child_session(ChildCommand::Create(command));
    silence(attempt(session.as_mut(), SHORT).await);
    publish_event(
        &world.js,
        &contract_jobs::evt_job_queued_v2_coords().expect("coords"),
        command_id,
        queued(job_id),
    )
    .await;
    attempt(session.as_mut(), WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("the late outcome is seen");
    seen.request().await;
    assert!(
        seen.requests.try_recv().is_err(),
        "the command was sent again"
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_finish_whose_completion_comes_between_two_attempts_then_it_is_delivered_and_not_sent_again()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut seen = commands_on(
        &world,
        contract_jobs::cmd_job_finish_v3_coords().expect("coords"),
    )
    .await;
    let (command_id, job_id) = (Uuid::now_v7(), Uuid::now_v7());
    let mut session = live.child_session(ChildCommand::Finish {
        command_id,
        job_id,
        run_id: Uuid::now_v7(),
    });
    silence(attempt(session.as_mut(), SHORT).await);
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
    attempt(session.as_mut(), WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("the late completion is seen");
    seen.request().await;
    assert!(
        seen.requests.try_recv().is_err(),
        "the finish was sent again"
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_rejection_that_comes_between_two_attempts_then_the_next_attempt_is_refused_and_sends_nothing()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut seen = commands_on(
        &world,
        contract_jobs::cmd_job_cancel_v3_coords().expect("coords"),
    )
    .await;
    let (command_id, job_id) = (Uuid::now_v7(), Uuid::now_v7());
    let mut session = live.child_session(ChildCommand::Cancel {
        command_id,
        job_id,
        run_id: Uuid::now_v7(),
    });
    silence(attempt(session.as_mut(), SHORT).await);
    publish_event(
        &world.js,
        &contract_jobs::evt_job_command_rejected_v1_coords().expect("coords"),
        command_id,
        JobCommandRejected {
            command_type: "cancel".to_owned(),
            job_id,
            parent_job_id: None,
            initial_job_id: None,
            reason_code: "job_terminal".to_owned(),
            message: "the job ended".to_owned(),
            params: None,
        },
    )
    .await;
    match attempt(session.as_mut(), WAIT).await.expect_err("refused") {
        Error::Refused(refusal) => {
            assert_eq!(refusal.operation, Operation::Cancel);
            assert_eq!(refusal.reason, "job_terminal");
        }
        other => panic!("{other:?}"),
    }
    seen.request().await;
    assert!(
        seen.requests.try_recv().is_err(),
        "the cancel was sent again"
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_no_outcome_for_two_attempts_then_each_attempt_sends_and_an_outcome_in_the_third_wait_ends_it()
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
    let mut session = live.child_session(ChildCommand::Create(command));
    silence(attempt(session.as_mut(), SHORT).await);
    silence(attempt(session.as_mut(), SHORT).await);
    let service = async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        publish_event(
            &world.js,
            &contract_jobs::evt_job_queued_v2_coords().expect("coords"),
            command_id,
            queued(job_id),
        )
        .await;
    };
    let (result, ()) = tokio::join!(attempt(session.as_mut(), WAIT), service);
    result.map_err(|e| e.to_string()).expect("delivered");
    for _ in 0..3 {
        seen.request().await;
    }
    assert!(seen.requests.try_recv().is_err());
}
