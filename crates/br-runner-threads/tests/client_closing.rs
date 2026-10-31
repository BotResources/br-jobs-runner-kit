mod support;

use br_runner_threads::test_double::{CommandBody, MemoryThreads};
use br_runner_threads::{ClosureKind, Error, MessageStatus};
use support::client_closing_x::*;
use support::client_x::*;
use support::*;

#[tokio::test]
async fn a_completed_closing_is_sent_as_a_completion_sealed_over_every_chunk() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 3).await;
    let closing = closing_of(&open, ClosureKind::Completed);

    let confirmed = threads
        .client()
        .close_message(&closing, Some(&handle))
        .await
        .expect("confirmed");

    let command = last_command(&threads);
    let CommandBody::Complete(complete) = &command.body else {
        panic!("not a completion: {:?}", command.body);
    };
    assert_eq!(command.id, closing.command_id);
    assert_eq!(complete.message_id, open.message_id.as_uuid());
    assert_eq!(complete.thread_id, open.thread_id);
    assert_eq!(complete.seal, seal_of(&threads));
    assert_eq!(complete.seal.last_seq, 2);
    assert_eq!(confirmed.command_id, closing.command_id);
    assert_eq!(confirmed.status, Some(MessageStatus::Completed));
}

#[tokio::test]
async fn budget_exhausted_failed_and_interrupted_closings_are_sent_as_failures_sealed_when_healthy()
{
    for kind in [
        ClosureKind::BudgetExhausted,
        ClosureKind::Failed,
        ClosureKind::Interrupted,
    ] {
        let threads = MemoryThreads::new();
        let (handle, open) = with_chunks(&threads, 2).await;
        let closing = closing_of(&open, kind);

        let confirmed = threads
            .client()
            .close_message(&closing, Some(&handle))
            .await
            .expect("confirmed");

        let command = last_command(&threads);
        let CommandBody::Fail(fail) = &command.body else {
            panic!("{kind:?} is not a failure: {:?}", command.body);
        };
        assert_eq!(fail.message_id, open.message_id.as_uuid());
        assert_eq!(fail.thread_id, open.thread_id);
        assert_eq!(fail.seal, Some(seal_of(&threads)), "{kind:?}");
        assert_eq!(confirmed.status, Some(MessageStatus::Failed));
    }
}

#[tokio::test]
async fn a_cancelled_closing_is_sent_as_a_cancel_sealed_when_healthy() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 2).await;
    let closing = closing_of(&open, ClosureKind::Cancelled);

    let confirmed = threads
        .client()
        .close_message(&closing, Some(&handle))
        .await
        .expect("confirmed");

    let command = last_command(&threads);
    let CommandBody::Cancel(cancel) = &command.body else {
        panic!("not a cancel: {:?}", command.body);
    };
    assert_eq!(cancel.seal, Some(seal_of(&threads)));
    assert_eq!(confirmed.status, Some(MessageStatus::Cancelled));
}

#[tokio::test]
async fn a_message_this_process_did_not_open_is_closed_by_its_id_with_no_seal() {
    for kind in NON_COMPLETED {
        let threads = MemoryThreads::new();
        let open = open_request(v7());
        let closing = closing_of(&open, kind);

        threads
            .client()
            .close_message(&closing, None)
            .await
            .expect("confirmed");

        let command = last_command(&threads);
        assert_eq!(command.id, closing.command_id);
        assert_eq!(seal_in(&command.body), None, "{kind:?}");
        let (message, thread) = match &command.body {
            CommandBody::Fail(f) => (f.message_id, f.thread_id),
            CommandBody::Cancel(c) => (c.message_id, c.thread_id),
            other => panic!("not a closing: {other:?}"),
        };
        assert_eq!(message, open.message_id.as_uuid());
        assert_eq!(thread, open.thread_id);
    }
}

#[tokio::test]
async fn a_handle_with_no_chunk_is_closed_with_no_seal() {
    for kind in NON_COMPLETED {
        let threads = MemoryThreads::new();
        let (handle, open) = opened(&threads.client()).await;

        threads
            .client()
            .close_message(&closing_of(&open, kind), Some(&handle))
            .await
            .expect("confirmed");

        assert_eq!(seal_in(&last_command(&threads).body), None, "{kind:?}");
    }
}

#[tokio::test]
async fn the_acting_user_of_a_closing_is_sent() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    let mut closing = closing_of(&open, ClosureKind::Completed);
    let acting = v7();
    closing.acting_user_id = Some(acting);

    threads
        .client()
        .close_message(&closing, Some(&handle))
        .await
        .expect("confirmed");

    let CommandBody::Complete(complete) = last_command(&threads).body else {
        panic!("not a completion");
    };
    assert_eq!(complete.acting_user_id, Some(acting));
}

#[tokio::test]
async fn a_completed_closing_without_a_handle_is_refused_before_sending() {
    let threads = MemoryThreads::new();
    let open = open_request(v7());

    let result = threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Completed), None)
        .await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert!(threads.operations().is_empty());
}

#[tokio::test]
async fn a_completed_closing_of_a_handle_with_no_chunk_is_refused_and_leaves_the_handle_usable() {
    let threads = MemoryThreads::new();
    let (handle, open) = opened(&threads.client()).await;
    let sent = threads.operations().len();

    let result = threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Completed), Some(&handle))
        .await;
    handle.keepalive().await.expect("the handle is still open");

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert_eq!(threads.operations().len(), sent + 1);
}

#[tokio::test]
async fn a_handle_of_another_message_is_refused() {
    let threads = MemoryThreads::new();
    let (handle, _) = with_chunks(&threads, 1).await;
    let other = open_request(v7());
    let sent = threads.operations().len();

    let result = threads
        .client()
        .close_message(&closing_of(&other, ClosureKind::Cancelled), Some(&handle))
        .await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert_eq!(threads.operations().len(), sent);
}

#[tokio::test]
async fn a_failed_closing_needs_a_detail_the_library_accepts() {
    let threads = MemoryThreads::new();
    let open = open_request(v7());
    let client = threads.client();
    let mut without = closing_of(&open, ClosureKind::Failed);
    without.detail = None;
    let mut empty = closing_of(&open, ClosureKind::Failed);
    empty.detail = Some(String::new());

    let missing = client.close_message(&without, None).await;
    let blank = client.close_message(&empty, None).await;

    assert!(
        matches!(missing, Err(Error::InvalidInput(_))),
        "{missing:?}"
    );
    assert!(matches!(blank, Err(Error::InvalidInput(_))), "{blank:?}");
    assert!(threads.operations().is_empty());
}

#[tokio::test]
async fn a_detail_the_library_refuses_is_refused_for_every_kind() {
    let threads = MemoryThreads::new();
    let open = open_request(v7());
    let client = threads.client();

    for kind in NON_COMPLETED {
        let mut closing = closing_of(&open, kind);
        closing.detail = Some(String::new());
        let result = client.close_message(&closing, None).await;
        assert!(matches!(result, Err(Error::InvalidInput(_))), "{kind:?}");
    }
    assert!(threads.operations().is_empty());
}

#[tokio::test]
async fn a_detail_the_library_accepts_is_accepted_for_every_kind() {
    for kind in NON_COMPLETED {
        let threads = MemoryThreads::new();
        let open = open_request(v7());
        let mut closing = closing_of(&open, kind);
        closing.detail = Some("because of the budget".to_owned());

        let result = threads.client().close_message(&closing, None).await;

        assert!(result.is_ok(), "{kind:?}: {result:?}");
    }
}

#[tokio::test]
async fn a_closing_command_id_that_is_not_a_uuidv7_is_refused_before_sending() {
    let threads = MemoryThreads::new();
    let open = open_request(v7());
    let mut closing = closing_of(&open, ClosureKind::Cancelled);
    closing.command_id = not_v7();

    let result = threads.client().close_message(&closing, None).await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert!(threads.operations().is_empty());
}
