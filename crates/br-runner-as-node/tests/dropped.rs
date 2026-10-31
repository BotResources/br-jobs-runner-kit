mod support;

use std::sync::Arc;

use br_runner_jobs::test_double::Answer;
use support::*;
use tokio::sync::Notify;

#[tokio::test]
async fn given_a_dropped_instance_when_the_node_waits_for_the_end_then_nothing_more_is_handed_over()
{
    let rig = rig();
    let (held, gone) = probe();
    let node = standard(probed_source(&rig.handle, held));
    let run = start(&node, rig.token.view()).await;
    run.give("q");
    let _ = the_create(&rig.script).await;

    drop(run);
    rig.token.handle().trigger();
    drop(node);
    within(gone).await.expect("the instance ended");

    assert_eq!(kinds(&rig.script), ["end_wait", "upload", "create"]);
}

#[tokio::test]
async fn given_a_dropped_instance_when_the_request_function_waits_then_the_wait_ends_and_no_call_is_made()
 {
    let rig = rig();
    let (held, gone) = probe();
    let waiting = Arc::new(Notify::new());
    let node = hung_request(probed_source(&rig.handle, held), Arc::clone(&waiting));
    let run = start(&node, rig.token.view()).await;
    run.give("q");
    started(&waiting).await;

    drop(run);
    drop(node);
    within(gone).await.expect("the instance ended");

    assert!(rig.script.calls().is_empty());
}

#[tokio::test]
async fn given_a_dropped_instance_when_the_caller_source_waits_then_the_wait_ends_and_no_call_is_made()
 {
    let rig = rig();
    let (held, gone) = probe();
    let waiting = Arc::new(Notify::new());
    let node = standard(hung_source(held, Arc::clone(&waiting)));
    let run = start(&node, rig.token.view()).await;
    run.give("q");
    started(&waiting).await;

    drop(run);
    drop(node);
    within(gone).await.expect("the instance ended");

    assert!(rig.script.calls().is_empty());
}

#[tokio::test]
async fn given_a_dropped_instance_when_the_response_function_waits_then_the_wait_ends_and_nothing_more_is_handed_over()
 {
    let rig = rig();
    let (held, gone) = probe();
    let waiting = Arc::new(Notify::new());
    let node = hung_response(probed_source(&rig.handle, held), Arc::clone(&waiting));
    let run = start(&node, rig.token.view()).await;
    run.give("q");
    let create = the_create(&rig.script).await;
    rig.script
        .end(create.job_id, Answer::Now(completed("done")));
    started(&waiting).await;

    drop(run);
    drop(node);
    within(gone).await.expect("the instance ended");

    assert_eq!(
        kinds(&rig.script),
        ["end_wait", "upload", "create", "finish"]
    );
}

#[tokio::test]
async fn given_no_input_ever_when_the_input_end_is_dropped_then_the_instance_ends_with_no_event_and_no_call()
 {
    let rig = rig();
    let (held, gone) = probe();
    let node = standard(probed_source(&rig.handle, held));
    let run = start(&node, rig.token.view()).await;
    let support::Running { input, mut events } = run;

    drop(input);
    drop(node);
    within(gone).await.expect("the instance ended");

    assert!(within(events.recv()).await.is_err());
    assert!(rig.script.calls().is_empty());
}
