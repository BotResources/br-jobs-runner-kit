mod support;

use std::sync::Arc;
use std::time::Duration;

use br_runner_threads::transport::Reply;
use br_runner_threads::{Closing, ClosureKind, Error};
use contract_threads::FailMessageV2;
use support::closings::*;
use support::*;

#[tokio::test]
async fn closings_may_coexist_and_the_handle_is_free_only_when_every_one_is_refused() {
    let f = fixture().await;
    let (c1, t1) = f.start(ClosureKind::Completed);
    commands_sent(&f.world.bus, 2).await;
    let (c2, t2) = f.start(ClosureKind::Failed);
    commands_sent(&f.world.bus, 3).await;
    f.world.bus.respond(c1.command_id, refusal(c1.command_id));
    assert!(matches!(t1.await.expect("task"), Err(Error::Refused(_))));
    assert_eq!(
        f.handle.keepalive().await,
        Err(Error::StreamClosed),
        "C2 still waits"
    );
    f.world.bus.respond(c2.command_id, refusal(c2.command_id));
    assert!(matches!(t2.await.expect("task"), Err(Error::Refused(_))));
    f.handle
        .keepalive()
        .await
        .expect("every sent closing was refused");
}

#[tokio::test]
async fn a_refused_closing_then_a_second_one_that_is_confirmed_spends_the_handle() {
    let f = fixture().await;
    let (c1, t1) = f.start(ClosureKind::Completed);
    commands_sent(&f.world.bus, 2).await;
    f.world.bus.respond(c1.command_id, refusal(c1.command_id));
    let _ = t1.await;
    f.world.bus.answer_with(|c| confirmed(c.command_id, None));
    f.close(ClosureKind::Failed).await.expect("closed");
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
async fn a_broken_handle_stays_broken_and_only_a_fail_closing_goes_out() {
    let f = fixture().await;
    f.world
        .bus
        .chunk_mode(br_runner_threads::transport::ChunkMode::Fail);
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamBroken));
    f.world
        .bus
        .chunk_mode(br_runner_threads::transport::ChunkMode::Ack);
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamBroken));
    assert_eq!(
        f.close(ClosureKind::Completed).await,
        Err(Error::StreamBroken)
    );
    assert_eq!(
        f.close(ClosureKind::BudgetExhausted).await,
        Err(Error::StreamBroken)
    );
    f.world.bus.answer_with(|c| confirmed(c.command_id, None));
    f.close(ClosureKind::Failed)
        .await
        .expect("a fail closing goes out on a broken handle");
    let last = f.world.bus.commands().pop().expect("the closing");
    let wire: FailMessageV2 = serde_json::from_value(last.payload).expect("wire");
    assert_eq!(wire.seal, None);
}

#[tokio::test]
async fn the_same_command_id_sent_again_counts_as_a_second_closing() {
    let f = fixture().await;
    let (first, t1) = f.start(ClosureKind::Completed);
    commands_sent(&f.world.bus, 2).await;
    let again = Closing { ..first.clone() };
    let task = {
        let (client, handle) = (f.world.client.clone(), Arc::clone(&f.handle));
        tokio::spawn(async move { client.close_message(&again, Some(&handle)).await.map(drop) })
    };
    commands_sent(&f.world.bus, 3).await;
    f.world
        .bus
        .respond(first.command_id, refusal(first.command_id));
    assert!(matches!(t1.await.expect("task"), Err(Error::Refused(_))));
    assert!(matches!(task.await.expect("task"), Err(Error::Refused(_))));
    f.handle.keepalive().await.expect("both were refused");
}

#[tokio::test]
async fn a_closing_with_no_chunk_goes_on_the_fail_command_and_the_handle_still_closes() {
    let world = world();
    let handle = open(&world).await;
    world.bus.answer_with(|c| confirmed(c.command_id, None));
    let closing = closing(&handle, world.thread, ClosureKind::Completed);
    world
        .client
        .close_message(&closing, Some(&handle))
        .await
        .expect("closed");
    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
async fn a_closing_waits_for_its_importer_deadline_and_loses_the_answer() {
    let f = fixture().await;
    f.world.bus.answer_with(|c| {
        if c.verb == "complete" {
            Reply::Never
        } else {
            confirmed(c.command_id, None)
        }
    });
    let mut closing = closing(&f.handle, f.world.thread, ClosureKind::Completed);
    closing.wait = Some(Duration::from_millis(40));
    let result = f
        .world
        .client
        .close_message(&closing, Some(&f.handle))
        .await;
    assert!(matches!(result, Err(Error::AnswerLost(_))), "{result:?}");
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamClosed));
}
