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
async fn every_closing_that_is_not_a_completion_is_sent_as_a_failure_that_names_its_closure() {
    for (kind, status) in [
        (ClosureKind::BudgetExhausted, MessageStatus::Completed),
        (ClosureKind::Failed, MessageStatus::Failed),
        (ClosureKind::Interrupted, MessageStatus::Failed),
        (ClosureKind::Cancelled, MessageStatus::Cancelled),
    ] {
        let threads = MemoryThreads::new();
        let (handle, open) = with_chunks(&threads, 2).await;
        let mut closing = closing_of(&open, kind);
        closing.detail = Some("the reason in words".to_owned());

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
        let reason = closure_in(&command.body);
        assert_eq!(kind_of(&reason), kind);
        assert_eq!(reason.detail(), Some("the reason in words"), "{kind:?}");
        assert_eq!(confirmed.status, Some(status), "{kind:?}");
    }
}

#[tokio::test]
async fn a_completed_closing_names_its_closure() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    let mut closing = closing_of(&open, ClosureKind::Completed);
    closing.detail = Some("all done".to_owned());

    threads
        .client()
        .close_message(&closing, Some(&handle))
        .await
        .expect("confirmed");

    let reason = closure_in(&last_command(&threads).body);
    assert_eq!(kind_of(&reason), ClosureKind::Completed);
    assert_eq!(reason.detail(), Some("all done"));
}

#[tokio::test]
async fn a_keepalive_is_in_the_seal_and_is_no_chunk_for_the_rules_that_need_one() {
    let threads = MemoryThreads::new();
    let (handle, open) = opened(&threads.client()).await;
    handle.keepalive().await.expect("taken");

    let completed = threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Completed), Some(&handle))
        .await;
    handle.entry(&notice("filler")).await.expect("still open");
    threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Failed), Some(&handle))
        .await
        .expect("confirmed");

    assert!(
        matches!(completed, Err(Error::InvalidInput(_))),
        "{completed:?}"
    );
    let sealed = seal_in(&last_command(&threads).body).expect("a seal");
    assert_eq!(sealed, seal_of(&threads));
    assert_eq!(sealed.last_seq, 1);
}

#[tokio::test]
async fn a_closing_after_nothing_but_keepalives_is_sent_with_no_seal() {
    let threads = MemoryThreads::new();
    let (handle, open) = opened(&threads.client()).await;
    handle.keepalive().await.expect("taken");

    threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Cancelled), Some(&handle))
        .await
        .expect("confirmed");

    assert_eq!(seal_in(&last_command(&threads).body), None);
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
        let CommandBody::Fail(fail) = &command.body else {
            panic!("not a failure: {:?}", command.body);
        };
        let (message, thread) = (fail.message_id, fail.thread_id);
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

    assert_eq!(
        missing,
        Err(Error::InvalidInput(
            "a Failed closing needs a detail".to_owned()
        ))
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
