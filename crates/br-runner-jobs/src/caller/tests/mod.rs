use std::time::Duration;

use futures::poll;
use serde_json::json;

use crate::book::tests::*;
use crate::caller::CreateChildInput;
use crate::error::Error;
use crate::model::{CallMode, PayloadForm, ResultRef};
use crate::queue::Removal;
use crate::test_double::{Call, ChildCommand};

const TEXT: &str = "The plain text of jobs, for an agent.";

fn input(child: u128, command: u128) -> CreateChildInput {
    CreateChildInput::new(
        id(child),
        id(command),
        "analyst",
        CallMode::Sync,
        PayloadForm::Kept,
    )
}

fn commands(fixture: &Fixture) -> Vec<ChildCommand> {
    fixture
        .jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Command(command) => Some(command),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn given_a_create_when_it_is_handed_over_then_parent_initial_passport_and_run_come_from_the_work()
 {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    let mut wanted = input(300, 301);
    wanted.config = Some(json!({ "k": "v" }));
    wanted.max_attempts = Some(3);
    handle
        .create_child(wanted)
        .expect("handed over")
        .await
        .expect("delivered");
    let ChildCommand::Create(create) = commands(&fixture).remove(0) else {
        panic!("a create is expected");
    };
    assert_eq!(create.job_id, id(300));
    assert_eq!(create.command_id, id(301));
    assert_eq!(create.runner_type, "analyst");
    assert_eq!(create.config, Some(json!({ "k": "v" })));
    assert_eq!(create.max_attempts, Some(3));
    assert_eq!(create.parent_job_id, id(JOB));
    assert_ne!(create.parent_job_id, id(PARENT));
    assert_eq!(create.initial_job_id, id(INITIAL));
    assert_eq!(create.run_id, id(RUN));
    assert_eq!(
        create.passport,
        serde_json::to_value(passport()).expect("json")
    );
}

#[tokio::test]
async fn given_an_upload_then_a_create_when_they_are_handed_over_then_the_upload_is_sent_first() {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    let upload = handle
        .upload_payload(id(300), content("payload"))
        .expect("upload");
    let create = handle.create_child(input(300, 301)).expect("create");
    assert_eq!(upload.await, Ok(()));
    assert_eq!(create.await, Ok(()));
    let calls = fixture.jobs.calls();
    let position = |wanted: fn(&Call) -> bool| calls.iter().position(wanted).expect("a call");
    let uploaded = position(|call| matches!(call, Call::Graphql { .. }));
    let created = position(|call| matches!(call, Call::Command(ChildCommand::Create(_))));
    assert!(uploaded < created);
}

#[tokio::test]
async fn given_finish_and_cancel_when_they_are_handed_over_after_the_create_then_they_carry_the_run_and_the_command_id()
 {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    handle
        .create_child(input(300, 301))
        .expect("create")
        .await
        .expect("created");
    handle
        .finish(id(300), id(302))
        .expect("finish")
        .await
        .expect("finished");
    handle
        .cancel(id(300), id(303))
        .expect("cancel")
        .await
        .expect("cancelled");
    let sent = commands(&fixture);
    assert_eq!(
        sent[1],
        ChildCommand::Finish {
            command_id: id(302),
            job_id: id(300),
            run_id: id(RUN)
        }
    );
    assert_eq!(
        sent[2],
        ChildCommand::Cancel {
            command_id: id(303),
            job_id: id(300),
            run_id: id(RUN)
        }
    );
}

#[tokio::test]
async fn given_a_refused_create_when_the_end_is_awaited_then_it_ends_removed_with_the_refusal_of_jobs()
 {
    let fixture = fixture().await;
    fixture.jobs.answer_commands(|| {
        Err(Error::refused(
            crate::error::Operation::Create,
            "runner_retired",
            TEXT,
        ))
    });
    let handle = fixture.book.caller_handle();
    let end = handle
        .create_child(input(300, 301))
        .expect("handed over")
        .await;
    let Err(Removal::Refused(Error::Refused(refusal))) = end else {
        panic!("a refusal is expected: {end:?}");
    };
    assert_eq!(refusal.reason, "runner_retired");
    assert_eq!(refusal.message, TEXT);
}

#[tokio::test]
async fn given_a_result_reference_when_it_is_read_then_the_call_carries_the_reference_the_keep_flag_and_the_work()
 {
    let fixture = fixture().await;
    fixture.queue.drained().await;
    fixture.jobs.push_graphql(Ok(json!({
        contract_jobs::graphql_roots::READ_RESULT_ROOT: {
            "success": true, "reason": null, "message": null,
            "content": { "message": "the answer", "attachments": [] },
        }
    })));
    let handle = fixture.book.caller_handle();
    let read = handle
        .read_result(ResultRef(id(310)), true)
        .await
        .expect("a Result");
    assert_eq!(read.message, "the answer");
    let Some(Call::Graphql { variables, .. }) = fixture.jobs.calls().into_iter().last() else {
        panic!("a GraphQL call is expected");
    };
    assert_eq!(variables["input"]["resultId"], json!(id(310)));
    assert_eq!(variables["input"]["keep"], json!(true));
    assert_eq!(variables["input"]["work"]["jobId"], json!(id(JOB)));
}

#[tokio::test]
async fn given_a_refused_result_read_when_it_is_awaited_then_refused_comes_back_and_nothing_is_read_again()
 {
    let fixture = fixture().await;
    fixture.queue.drained().await;
    fixture.jobs.push_graphql(Ok(json!({
        contract_jobs::graphql_roots::READ_RESULT_ROOT:
            { "success": false, "reason": "gone", "message": TEXT }
    })));
    let handle = fixture.book.caller_handle();
    let error = handle
        .read_result(ResultRef(id(310)), false)
        .await
        .expect_err("refused");
    assert!(error.is_refusal());
    let reads = fixture
        .jobs
        .calls()
        .into_iter()
        .filter(|call| matches!(call, Call::Graphql { .. }))
        .count();
    assert_eq!(reads, 1);
}

#[tokio::test]
async fn given_a_result_read_dropped_before_jobs_answers_when_the_handle_goes_on_then_nothing_is_split_or_repeated()
 {
    let fixture = fixture().await;
    fixture.queue.drained().await;
    fixture.jobs.pause();
    let handle = fixture.book.caller_handle();
    let mut read = Box::pin(handle.read_result(ResultRef(id(310)), false));
    assert!(poll!(read.as_mut()).is_pending());
    drop(read);
    let create = handle.create_child(input(300, 301)).expect("create");
    fixture.jobs.resume();
    assert_eq!(create.await, Ok(()));
    let creates = commands(&fixture).len();
    let reads = fixture
        .jobs
        .calls()
        .into_iter()
        .filter(|call| matches!(call, Call::Graphql { .. }))
        .count();
    assert_eq!((creates, reads), (1, 0));
}

#[tokio::test]
async fn given_hand_overs_when_the_handle_returns_then_the_writes_are_already_in_the_queue() {
    let fixture = fixture().await;
    fixture.queue.drained().await;
    fixture.jobs.pause();
    let handle = fixture.book.caller_handle();
    let _upload = handle
        .upload_payload(id(300), content("p"))
        .expect("upload");
    let _create = handle.create_child(input(300, 301)).expect("create");
    let waiting = fixture.queue.state().waiting;
    assert_eq!(waiting.len(), 2);
    fixture.jobs.resume();
    tokio::time::timeout(Duration::from_secs(5), fixture.queue.drained())
        .await
        .expect("drained");
}
