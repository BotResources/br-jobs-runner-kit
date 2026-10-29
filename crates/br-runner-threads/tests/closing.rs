mod support;

use std::sync::Arc;
use std::time::Duration;

use br_llm_messages::ClosureReason;
use br_runner_threads::transport::{Answer, ChunkMode, Reply};
use br_runner_threads::{Closing, ClosureKind, Error, MessageStatus, Refusal};
use contract_threads::{Chunk, ChunkFrame, CompleteMessageV2, FailMessageV2};
use serde_json::json;
use support::*;
use tokio::sync::Notify;

fn standalone(world: &World, kind: ClosureKind, detail: Option<&str>) -> Closing {
    Closing {
        command_id: v7(),
        thread: world.thread,
        message_id: v7(),
        kind,
        detail: detail.map(str::to_owned),
        acting_user_id: Some(v7()),
        wait: None,
    }
}

fn closure_of(payload: &serde_json::Value) -> ClosureReason {
    serde_json::from_value(payload["closure"].clone()).expect("the library's closure reason")
}

#[tokio::test]
async fn completed_and_budget_exhausted_close_with_a_completion_that_carries_the_seal() {
    for (kind, wanted) in [
        (ClosureKind::Completed, ClosureReason::completed()),
        (
            ClosureKind::BudgetExhausted,
            ClosureReason::budget_exhausted(),
        ),
    ] {
        let world = world();
        let handle = open(&world).await;
        handle.keepalive().await.expect("chunk");
        handle.entry(&notice()).await.expect("chunk");
        let closing = closing(&handle, world.thread, kind);
        let confirmed = world
            .client
            .close_message(&closing, Some(&handle))
            .await
            .expect("closed");
        assert_eq!(confirmed.command_id, closing.command_id);
        let sent = world.bus.commands().pop().expect("the closing");
        assert_eq!(
            (sent.aggregate, sent.verb, sent.version),
            ("message", "complete", 2)
        );
        let wire: CompleteMessageV2 = serde_json::from_value(sent.payload.clone()).expect("wire");
        assert_eq!(wire.message_id, handle.message_id());
        assert_eq!(wire.thread_id, world.thread.as_uuid());
        assert_eq!(wire.seal.last_seq, 1);
        let chunks: Vec<Chunk> = world
            .bus
            .chunks()
            .into_iter()
            .map(|c| {
                serde_json::from_value::<ChunkFrame>(c.frame)
                    .expect("frame")
                    .chunk
            })
            .collect();
        assert_eq!(
            wire.seal.hash,
            contract_threads::seal_hash(chunks.iter()).expect("hash")
        );
        assert_eq!(closure_of(&sent.payload), wanted);
    }
}

#[tokio::test]
async fn failed_interrupted_and_cancelled_close_with_a_fail_command_that_carries_no_seal() {
    for kind in [
        ClosureKind::Failed,
        ClosureKind::Interrupted,
        ClosureKind::Cancelled,
    ] {
        let world = world();
        let handle = open(&world).await;
        handle.keepalive().await.expect("chunk");
        let closing = closing(&handle, world.thread, kind);
        world
            .client
            .close_message(&closing, Some(&handle))
            .await
            .expect("closed");
        let sent = world.bus.commands().pop().expect("the closing");
        assert_eq!(
            (sent.aggregate, sent.verb, sent.version),
            ("message", "fail", 2)
        );
        let wire: FailMessageV2 = serde_json::from_value(sent.payload).expect("wire");
        assert_eq!(wire.seal, None);
    }
}

#[tokio::test]
async fn a_message_this_process_did_not_open_is_closed_by_its_id_without_a_handle() {
    let world = world();
    for kind in [
        ClosureKind::Failed,
        ClosureKind::Interrupted,
        ClosureKind::Cancelled,
    ] {
        let closing = standalone(&world, kind, Some("restart"));
        world
            .client
            .close_message(&closing, None)
            .await
            .expect("closed by id");
        let sent = world.bus.commands().pop().expect("the closing");
        assert_eq!(sent.verb, "fail");
        let wire: FailMessageV2 = serde_json::from_value(sent.payload.clone()).expect("wire");
        assert_eq!(wire.message_id, closing.message_id);
        assert_eq!(wire.acting_user_id, closing.acting_user_id);
        assert_eq!(closure_of(&sent.payload).detail(), Some("restart"));
    }
}

#[tokio::test]
async fn completed_and_budget_exhausted_need_the_handle() {
    let world = world();
    for kind in [ClosureKind::Completed, ClosureKind::BudgetExhausted] {
        let result = world
            .client
            .close_message(&standalone(&world, kind, None), None)
            .await;
        assert!(matches!(result, Err(Error::InvalidInput(_))));
    }
    assert!(world.bus.commands().is_empty());
}

#[tokio::test]
async fn a_stream_with_no_chunk_has_no_seal_so_the_reason_goes_on_the_fail_command() {
    for kind in [ClosureKind::Completed, ClosureKind::BudgetExhausted] {
        let world = world();
        let handle = open(&world).await;
        let closing = closing(&handle, world.thread, kind);
        world
            .client
            .close_message(&closing, Some(&handle))
            .await
            .expect("closed");
        let sent = world.bus.commands().pop().expect("the closing");
        assert_eq!(sent.verb, "fail");
        let wire: FailMessageV2 = serde_json::from_value(sent.payload.clone()).expect("wire");
        assert_eq!(wire.seal, None);
        assert_eq!(closure_of(&sent.payload).detail(), None);
        assert!(sent.payload["closure"]["type"].as_str().is_some());
    }
}

#[tokio::test]
async fn the_detail_is_checked_as_the_library_checks_it() {
    let world = world();
    let bad = [
        (ClosureKind::Failed, None),
        (ClosureKind::Failed, Some("   ")),
        (ClosureKind::Cancelled, Some("")),
        (ClosureKind::Completed, Some("  ")),
    ];
    for (kind, detail) in bad {
        let result = world
            .client
            .close_message(&standalone(&world, kind, detail), None)
            .await;
        assert!(
            matches!(result, Err(Error::InvalidInput(_))),
            "{kind:?} {detail:?}"
        );
    }
    assert!(world.bus.commands().is_empty());
    let handle = open(&world).await;
    handle.keepalive().await.expect("chunk");
    let mut closing = closing(&handle, world.thread, ClosureKind::Completed);
    closing.detail = Some("all done".to_owned());
    world
        .client
        .close_message(&closing, Some(&handle))
        .await
        .expect("closed");
    let sent = world.bus.commands().pop().expect("the closing");
    assert_eq!(closure_of(&sent.payload).detail(), Some("all done"));
}

#[tokio::test]
async fn a_handle_of_another_message_is_refused() {
    let world = world();
    let handle = open(&world).await;
    let mut closing = closing(&handle, world.thread, ClosureKind::Failed);
    closing.message_id = v7();
    let result = world.client.close_message(&closing, Some(&handle)).await;
    assert!(matches!(result, Err(Error::InvalidInput(_))));
}

#[tokio::test]
async fn the_closing_goes_after_every_chunk_that_took_a_number_and_after_its_acknowledgement() {
    let world = world();
    let handle = Arc::new(open(&world).await);
    let release = Arc::new(Notify::new());
    world.bus.chunk_mode(ChunkMode::Hold(Arc::clone(&release)));
    let sending = {
        let handle = Arc::clone(&handle);
        tokio::spawn(async move { handle.keepalive().await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    let closing = closing(&handle, world.thread, ClosureKind::Completed);
    let closer = {
        let (client, handle) = (world.client.clone(), Arc::clone(&handle));
        tokio::spawn(async move { client.close_message(&closing, Some(&handle)).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        world.bus.commands().len(),
        1,
        "only the opening: the chunk is not acknowledged"
    );
    release.notify_waiters();
    sending.await.expect("task").expect("chunk acknowledged");
    closer.await.expect("task").expect("closed");
    let sent = world.bus.commands().pop().expect("the closing");
    assert_eq!(sent.payload["seal"]["last_seq"], 0);
}

#[tokio::test]
async fn a_confirmation_can_carry_failed_and_a_refusal_gives_its_code() {
    let world = world();
    world
        .bus
        .answer_with(|c| confirmed(c.command_id, Some(MessageStatus::Failed)));
    let handle = open(&world).await;
    handle.keepalive().await.expect("chunk");
    let closing = closing(&handle, world.thread, ClosureKind::Completed);
    let answered = world
        .client
        .close_message(&closing, Some(&handle))
        .await
        .expect("closed");
    assert_eq!(answered.status, Some(MessageStatus::Failed));

    let world = self::world();
    let handle = open(&world).await;
    handle.keepalive().await.expect("chunk");
    world.bus.answer_with(|c| {
        Reply::Answer(Answer::Refused(Refusal {
            code: "STREAM_MISMATCH".to_owned(),
            params: json!({ "error": "hash" })
                .as_object()
                .cloned()
                .expect("params"),
            command_id: Some(c.command_id),
        }))
    });
    let closing = self::closing(&handle, world.thread, ClosureKind::Completed);
    match world.client.close_message(&closing, Some(&handle)).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.code, "STREAM_MISMATCH");
            assert_eq!(refusal.params["error"], "hash");
            assert_eq!(refusal.command_id, Some(closing.command_id));
        }
        other => panic!("a refusal, not {other:?}"),
    }
}
