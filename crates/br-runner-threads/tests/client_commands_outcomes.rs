mod support;

use br_runner_threads::test_double::{MemoryThreads, Outcome};
use br_runner_threads::{Cause, Error, Failure, MessageStatus};
use support::client_x::*;
use support::*;

#[tokio::test]
async fn a_command_gives_back_the_status_that_threads_confirmed() {
    for status in [
        MessageStatus::Streaming,
        MessageStatus::Completed,
        MessageStatus::Failed,
        MessageStatus::Cancelled,
    ] {
        let threads = MemoryThreads::new();
        threads.answer_commands(move |command| {
            Ok(Outcome {
                command_id: command.id,
                status: Some(status),
            })
        });
        let message = ai_message(v7());

        let confirmed = threads
            .client()
            .write_message(&message)
            .await
            .expect("confirmed");

        assert_eq!(confirmed.status, Some(status));
        assert_eq!(confirmed.command_id, message.command_id);
    }
}

#[tokio::test]
async fn an_unavailable_outcome_reaches_the_caller_as_unavailable() {
    let threads = MemoryThreads::new();
    let failure = Failure::new(Cause::NoListener, "nobody listens");
    let expected = failure.clone();
    threads.answer_commands(move |_| Err(Error::Unavailable(failure.clone())));
    let client = threads.client();

    let created = client.create_thread(&new_thread()).await;
    let written = client.write_message(&ai_message(v7())).await;
    let opened = client.open_stream(&open_request(v7())).await;

    assert_eq!(created, Err(Error::Unavailable(expected.clone())));
    assert_eq!(written, Err(Error::Unavailable(expected.clone())));
    assert!(matches!(opened, Err(Error::Unavailable(f)) if f == expected));
}

#[tokio::test]
async fn a_command_that_got_no_outcome_in_time_reaches_the_caller_as_lost() {
    let threads = MemoryThreads::new();
    let failure = Failure::new(Cause::WaitFired, "no outcome");
    let expected = failure.clone();
    threads.answer_commands(move |_| Err(Error::Lost(failure.clone())));
    let client = threads.client();

    let created = client.create_thread(&new_thread()).await;
    let opened = client.open_stream(&open_request(v7())).await;

    assert_eq!(created, Err(Error::Lost(expected.clone())));
    assert!(matches!(opened, Err(Error::Lost(f)) if f == expected));
}

#[tokio::test]
async fn a_refused_outcome_reaches_the_caller_with_its_code_and_command_id() {
    let threads = MemoryThreads::new();
    let refusal = refusal_error();
    let expected = refusal.clone();
    threads.answer_commands(move |_| Err(refusal.clone()));
    let client = threads.client();

    let created = client.create_thread(&new_thread()).await;
    let written = client.write_message(&ai_message(v7())).await;
    let opened = client.open_stream(&open_request(v7())).await;

    assert_eq!(created, Err(expected.clone()));
    assert_eq!(written, Err(expected.clone()));
    assert!(matches!(opened, Err(e) if e == expected));
}

#[tokio::test]
async fn a_credential_or_unauthenticated_error_reaches_the_caller_unchanged() {
    let threads = MemoryThreads::new();
    threads.answer_commands(|_| Err(Error::Unauthenticated));

    let result = threads.client().create_thread(&new_thread()).await;

    assert_eq!(result, Err(Error::Unauthenticated));
}

#[tokio::test]
async fn an_unreadable_outcome_reaches_the_caller_as_unreadable() {
    let threads = MemoryThreads::new();
    threads.answer_commands(|_| Err(Error::Unreadable("garbled".to_owned())));

    let result = threads.client().write_message(&ai_message(v7())).await;

    assert_eq!(result, Err(Error::Unreadable("garbled".to_owned())));
}

#[tokio::test]
async fn a_failed_opening_gives_no_handle_and_sends_no_chunk() {
    let threads = MemoryThreads::new();
    threads.answer_commands(|_| Err(Error::Unavailable(Failure::new(Cause::Transport, "down"))));
    let result = threads.client().open_stream(&open_request(v7())).await;

    assert!(result.is_err());
    assert!(threads.chunks().is_empty());
}
