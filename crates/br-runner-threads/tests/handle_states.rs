mod support;

use br_runner_threads::transport::{ChunkMode, Reply};
use br_runner_threads::{ClosureKind, Error};
use support::closings::*;
use support::*;

#[tokio::test]
async fn while_a_closing_waits_the_handle_takes_no_chunk() {
    let f = fixture().await;
    let (_, task) = f.start(ClosureKind::Completed);
    commands_sent(&f.world.bus, 2).await;
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamClosed));
    task.abort();
}

#[tokio::test]
async fn a_refused_closing_frees_the_handle_and_a_second_closing_with_no_new_chunk_carries_the_same_seal()
 {
    let f = fixture().await;
    let (first, task) = f.start(ClosureKind::Completed);
    commands_sent(&f.world.bus, 2).await;
    f.world
        .bus
        .respond(first.command_id, refusal(first.command_id));
    assert!(matches!(task.await.expect("task"), Err(Error::Refused(_))));
    f.world.bus.answer_with(|c| confirmed(c.command_id, None));
    f.close(ClosureKind::Completed)
        .await
        .expect("second closing");
    let seals = seals_sent(&f.world);
    assert_eq!(seals.len(), 2);
    assert_eq!(seals[0], seals[1]);
}

#[tokio::test]
async fn a_chunk_written_after_a_refused_closing_is_covered_by_the_next_seal() {
    let f = fixture().await;
    let (first, task) = f.start(ClosureKind::Completed);
    commands_sent(&f.world.bus, 2).await;
    f.world
        .bus
        .respond(first.command_id, refusal(first.command_id));
    let _ = task.await;
    f.handle
        .keepalive()
        .await
        .expect("a refused closing frees the handle");
    f.world.bus.answer_with(|c| confirmed(c.command_id, None));
    f.close(ClosureKind::Completed)
        .await
        .expect("second closing");
    let seals = seals_sent(&f.world);
    assert_eq!((seals[0].last_seq, seals[1].last_seq), (0, 1));
}

#[tokio::test]
async fn a_lost_answer_closes_the_handle_for_good_but_a_closing_may_be_retried() {
    let f = fixture().await;
    f.world.bus.answer_with(|c| {
        if c.verb == "complete" {
            Reply::Lost
        } else {
            confirmed(c.command_id, None)
        }
    });
    assert!(matches!(
        f.close(ClosureKind::Completed).await,
        Err(Error::AnswerLost(_))
    ));
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamClosed));
    f.world.bus.answer_with(|c| confirmed(c.command_id, None));
    f.close(ClosureKind::Completed)
        .await
        .expect("the closing is retried with the same seal");
}

#[tokio::test]
async fn an_unreadable_answer_closes_the_handle_for_good() {
    let f = fixture().await;
    f.world.bus.answer_with(|c| {
        if c.verb == "complete" {
            Reply::Unreadable("garbled".to_owned())
        } else {
            confirmed(c.command_id, None)
        }
    });
    assert!(matches!(
        f.close(ClosureKind::Completed).await,
        Err(Error::Unreadable(_))
    ));
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
async fn an_abandoned_closing_closes_the_handle_for_good() {
    let f = fixture().await;
    let (_, task) = f.start(ClosureKind::Completed);
    commands_sent(&f.world.bus, 2).await;
    task.abort();
    let _ = task.await;
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
async fn a_closing_that_never_left_does_not_close_the_handle() {
    let f = fixture().await;
    f.world.bus.answer_with(|c| {
        if c.verb == "complete" {
            Reply::Unavailable("bus down".to_owned())
        } else {
            confirmed(c.command_id, None)
        }
    });
    assert!(matches!(
        f.close(ClosureKind::Completed).await,
        Err(Error::Unavailable(_))
    ));
    f.handle
        .keepalive()
        .await
        .expect("nothing was sent, the handle is free");
}

#[tokio::test]
async fn a_confirmed_closing_spends_the_handle() {
    let f = fixture().await;
    f.world.bus.answer_with(|c| confirmed(c.command_id, None));
    f.close(ClosureKind::Completed).await.expect("closed");
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamClosed));
    assert_eq!(f.handle.entry(&notice()).await, Err(Error::StreamClosed));
    assert_eq!(f.close(ClosureKind::Failed).await, Err(Error::StreamClosed));
    assert_eq!(
        f.close(ClosureKind::Completed).await,
        Err(Error::StreamClosed)
    );
}

#[tokio::test]
async fn a_broken_handle_with_a_waiting_fail_closing_reports_broken_not_closed() {
    let f = fixture().await;
    f.world.bus.chunk_mode(ChunkMode::Fail);
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamBroken));
    f.world.bus.chunk_mode(ChunkMode::Ack);
    let (_, task) = f.start(ClosureKind::Failed);
    commands_sent(&f.world.bus, 2).await;
    assert_eq!(f.handle.keepalive().await, Err(Error::StreamBroken));
    task.abort();
}
