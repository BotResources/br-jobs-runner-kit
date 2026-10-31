mod support;

use br_graph_contract::Event;
use br_runner_jobs::test_double::{Answer, MemoryCall, result_ref};
use br_runner_jobs::{CallMode, Error as JobsError, PayloadForm, Removal, SyncEnd};
use support::*;
use uuid::Uuid;

fn completed(message: &str) -> SyncEnd {
    SyncEnd::Completed {
        inline: Some(content(message)),
        result_ref: None,
    }
}

async fn a_refusal(reason: &str) -> JobsError {
    use br_runner_jobs::test_double::ScriptedJobs;
    let jobs = ScriptedJobs::new("analyst", "pod-1");
    let body = format!(r#"{{"version":2,"reason":"{reason}","retry_after_ms":250}}"#);
    jobs.push_claim_reply(Ok(body.into_bytes()));
    match jobs
        .client()
        .claim(Uuid::now_v7(), std::time::Duration::from_secs(1))
        .await
    {
        Err(error @ JobsError::Refused(_)) => error,
        other => panic!("expected a refusal, got {:?}", other.map(|_| ())),
    }
}

#[tokio::test]
async fn given_a_kept_payload_when_the_runner_completes_then_the_answer_goes_in_the_event_result_after_the_finish()
 {
    let rig = rig();
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("count the tickets");

    let create = the_create(&rig.script).await;
    rig.script
        .end(create.job_id, Answer::Now(completed("42 tickets")));
    let update = update_of(run.event().await);

    assert_eq!(update.answer.as_deref(), Some("42 tickets"));
    assert_eq!(
        kinds(&rig.script),
        ["end_wait", "upload", "create", "finish"]
    );
    let calls = rig.script.calls();
    assert!(matches!(
        &calls[1],
        MemoryCall::Upload { job_id, content: c }
            if *job_id == create.job_id && c.message == "count the tickets"
    ));
    assert!(matches!(
        &calls[3],
        MemoryCall::Finish { job_id, .. } if *job_id == create.job_id
    ));
    assert_eq!(create.mode, CallMode::Sync);
    assert_eq!(create.payload, PayloadForm::Kept);
    assert_eq!(create.runner_type, "analyst");
}

#[tokio::test]
async fn given_a_result_reference_when_the_runner_completes_then_the_reference_passes_on_unread() {
    let rig = rig();
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let create = the_create(&rig.script).await;
    let reference = Uuid::now_v7();

    rig.script.end(
        create.job_id,
        Answer::Now(SyncEnd::Completed {
            inline: None,
            result_ref: Some(result_ref(reference)),
        }),
    );
    let update = update_of(run.event().await);

    assert_eq!(update.note, Some(format!("ref:{reference}")));
    assert!(!kinds(&rig.script).contains(&"read"));
}

#[tokio::test]
async fn given_a_failed_end_when_the_response_function_marks_it_then_the_event_result_carries_it_and_no_finish_is_handed_over()
 {
    let rig = rig();
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let create = the_create(&rig.script).await;

    rig.script.end(
        create.job_id,
        Answer::Now(SyncEnd::Failed {
            report: None,
            message: "the analyst broke".to_owned(),
        }),
    );
    let update = update_of(run.event().await);

    assert_eq!(update.note.as_deref(), Some("failed:the analyst broke"));
    assert_eq!(kinds(&rig.script), ["end_wait", "upload", "create"]);
}

#[tokio::test]
async fn given_a_cancelled_end_when_the_response_function_marks_it_then_the_event_result_carries_it_and_no_cancel_is_handed_over()
 {
    let rig = rig();
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let create = the_create(&rig.script).await;

    rig.script.end(
        create.job_id,
        Answer::Now(SyncEnd::Cancelled {
            message: "stopped".to_owned(),
        }),
    );
    let update = update_of(run.event().await);

    assert_eq!(update.note.as_deref(), Some("cancelled:stopped"));
    assert_eq!(kinds(&rig.script), ["end_wait", "upload", "create"]);
}

#[tokio::test]
async fn given_a_refused_create_when_the_node_waits_then_the_response_function_gets_the_refusal_and_no_cancel_or_finish_is_handed_over()
 {
    let rig = rig();
    rig.script.create(Answer::Now(Err(Removal::Refused(
        a_refusal("unknown_runner_type").await,
    ))));
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;

    run.give("q");
    let update = update_of(run.event().await);

    assert_eq!(update.note.as_deref(), Some("refused:unknown_runner_type"));
    assert_eq!(kinds(&rig.script), ["end_wait", "upload", "create"]);
}

#[tokio::test]
async fn given_a_refused_upload_when_it_comes_in_the_end_of_the_create_then_the_outcome_is_the_refusal()
 {
    let rig = rig();
    rig.script.create(Answer::Late(Err(Removal::Refused(
        a_refusal("payload_too_big").await,
    ))));
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let _ = the_create(&rig.script).await;

    rig.script.release();
    let update = update_of(run.event().await);

    assert_eq!(update.note.as_deref(), Some("refused:payload_too_big"));
}

#[tokio::test]
async fn given_a_delivered_create_when_no_end_comes_then_the_node_keeps_waiting_and_hands_over_nothing_more()
 {
    let rig = rig();
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let _ = the_create(&rig.script).await;

    let still_waiting = stays_pending(run.events.recv()).await;

    assert!(still_waiting);
    assert_eq!(kinds(&rig.script), ["end_wait", "upload", "create"]);
}

#[tokio::test]
async fn given_an_end_that_comes_before_the_late_delivery_of_the_create_then_the_end_settles_the_call()
 {
    let rig = rig();
    rig.script.create(Answer::Late(Ok(())));
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");
    let create = the_create(&rig.script).await;

    rig.script
        .end(create.job_id, Answer::Now(completed("fast")));
    let update = update_of(run.event().await);

    assert_eq!(update.answer.as_deref(), Some("fast"));
}

#[tokio::test]
async fn given_the_node_never_sends_the_event_refused_when_every_outcome_is_tried_then_none_is_a_refused_event()
 {
    let rig = rig();
    rig.script
        .create(Answer::Now(Err(Removal::Refused(a_refusal("busy").await))));
    let node = standard(source_of(&rig.handle));
    let mut run = start(&node, rig.token.view()).await;
    run.give("q");

    let event = run.event().await;

    assert!(!matches!(event, Event::Refused(_)));
}
