mod support;

use br_runner_threads::test_double::{CommandBody, MemoryThreads, Outcome};
use br_runner_threads::{ClosureKind, Error, MessageStatus};
use support::client_closing_x::*;
use support::client_x::*;
use support::*;

#[tokio::test]
async fn a_closing_that_got_no_outcome_in_time_leaves_the_handle_closed() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    threads.answer_commands(|_| Err(lost()));

    let result = threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Cancelled), Some(&handle))
        .await;

    assert!(matches!(result, Err(Error::Lost(_))), "{result:?}");
    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
async fn a_closing_with_an_unreadable_outcome_leaves_the_handle_closed() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    threads.answer_commands(|_| Err(Error::Unreadable("garbled".to_owned())));

    let result = threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Cancelled), Some(&handle))
        .await;

    assert!(matches!(result, Err(Error::Unreadable(_))), "{result:?}");
    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
async fn abandoning_a_closing_leaves_the_handle_closed_because_threads_may_have_applied_it() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    let before = threads.commands().len();
    threads.pause();
    let client = threads.client();
    let closing = closing_of(&open, ClosureKind::Cancelled);
    let handle = std::sync::Arc::new(handle);
    let shared = std::sync::Arc::clone(&handle);
    let task = tokio::spawn(async move { client.close_message(&closing, Some(&shared)).await });
    eventually("the closing reached Threads", || {
        threads.commands().len() == before + 1
    })
    .await;

    task.abort();
    let _ = task.await;
    threads.resume();

    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
async fn a_closing_that_was_not_sent_frees_the_handle_and_can_be_tried_again() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 2).await;
    let client = threads.client();
    threads.answer_commands(|_| Err(unavailable()));
    let closing = closing_of(&open, ClosureKind::Completed);

    let first = client.close_message(&closing, Some(&handle)).await;
    handle.keepalive().await.expect("the handle is free again");
    threads.answer_commands(|command| {
        Ok(Outcome {
            command_id: command.id,
            status: Some(MessageStatus::Completed),
        })
    });
    let retry = client
        .close_message(&closing_of(&open, ClosureKind::Completed), Some(&handle))
        .await;

    assert!(matches!(first, Err(Error::Unavailable(_))), "{first:?}");
    let confirmed = retry.expect("the retry is confirmed");
    assert_eq!(confirmed.status, Some(MessageStatus::Completed));
    let CommandBody::Complete(complete) = last_command(&threads).body else {
        panic!("not a completion");
    };
    assert_eq!(complete.seal.last_seq, 2);
    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
async fn a_refused_closing_frees_the_handle_again() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 2).await;
    let refusal = refusal_error();
    threads.answer_commands(move |_| Err(refusal.clone()));

    let result = threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Completed), Some(&handle))
        .await;
    let after = handle.keepalive().await;

    assert!(matches!(result, Err(Error::Refused(_))), "{result:?}");
    assert_eq!(after, Ok(()));
}

#[tokio::test]
async fn a_completed_closing_on_a_broken_handle_is_refused_as_broken_and_sends_nothing() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    threads.answer_chunks(|_, _, _| Err(lost()));
    handle.keepalive().await.expect_err("broken");
    let sent = threads.commands().len();

    let result = threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Completed), Some(&handle))
        .await;

    assert_eq!(result, Err(Error::StreamBroken));
    assert_eq!(threads.commands().len(), sent);
}

#[tokio::test]
async fn a_broken_handle_is_closed_by_its_id_with_no_seal_for_the_other_reasons() {
    for kind in NON_COMPLETED {
        let threads = MemoryThreads::new();
        let (handle, open) = with_chunks(&threads, 2).await;
        threads.answer_chunks(|_, _, _| Err(lost()));
        handle.keepalive().await.expect_err("broken");
        threads.answer_chunks(|_, _, _| Ok(()));

        let confirmed = threads
            .client()
            .close_message(&closing_of(&open, kind), Some(&handle))
            .await;

        assert!(confirmed.is_ok(), "{kind:?}: {confirmed:?}");
        assert_eq!(seal_in(&last_command(&threads).body), None, "{kind:?}");
    }
}

#[tokio::test]
async fn a_failure_closing_after_a_lost_closing_is_sent_by_id_with_no_seal() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 2).await;
    let client = threads.client();
    threads.answer_commands(|_| Err(lost()));
    client
        .close_message(&closing_of(&open, ClosureKind::Cancelled), Some(&handle))
        .await
        .expect_err("lost");
    threads.answer_commands(|command| {
        Ok(Outcome {
            command_id: command.id,
            status: Some(MessageStatus::Failed),
        })
    });

    client
        .close_message(&closing_of(&open, ClosureKind::Failed), Some(&handle))
        .await
        .expect("confirmed");

    assert_eq!(seal_in(&last_command(&threads).body), None);
}
