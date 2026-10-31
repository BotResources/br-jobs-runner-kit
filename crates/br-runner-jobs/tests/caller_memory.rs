use std::time::Duration;

use br_runner_jobs::test_double::{Answer, MemoryCall, memory_caller, result_ref};
use br_runner_jobs::{CallMode, Content, CreateChildInput, Error, PayloadForm, Removal, SyncEnd};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn content(message: &str) -> Content {
    Content {
        message: message.to_owned(),
        attachments: Vec::new(),
    }
}

fn input(child: u128) -> CreateChildInput {
    CreateChildInput::new(
        id(child),
        id(child + 1000),
        "analyst",
        CallMode::Sync,
        PayloadForm::Kept,
    )
}

fn cancelled(message: &str) -> SyncEnd {
    SyncEnd::Cancelled {
        message: message.to_owned(),
    }
}

async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("it ended")
}

async fn stays_pending<T>(future: impl std::future::Future<Output = T>) -> bool {
    tokio::time::timeout(Duration::from_millis(50), future)
        .await
        .is_err()
}

#[tokio::test]
async fn given_nothing_scripted_when_the_calls_are_made_then_every_write_is_delivered_and_the_calls_are_recorded_in_order()
 {
    let (handle, script) = memory_caller();
    within(handle.upload_payload(id(1), content("p")).expect("upload"))
        .await
        .expect("delivered");
    within(handle.create_child(input(1)).expect("create"))
        .await
        .expect("delivered");
    within(handle.finish(id(1), id(2001)).expect("finish"))
        .await
        .expect("delivered");
    within(handle.cancel(id(1), id(2002)).expect("cancel"))
        .await
        .expect("delivered");
    assert_eq!(
        script.calls(),
        vec![
            MemoryCall::Upload {
                job_id: id(1),
                content: content("p")
            },
            MemoryCall::Create(input(1)),
            MemoryCall::Finish {
                job_id: id(1),
                command_id: id(2001)
            },
            MemoryCall::Cancel {
                job_id: id(1),
                command_id: id(2002)
            },
        ]
    );
}

#[tokio::test]
async fn given_a_late_create_when_it_is_handed_over_then_its_end_waits_for_the_release() {
    let (handle, script) = memory_caller();
    script.create(Answer::Late(Ok(())));
    let mut end = handle.create_child(input(1)).expect("create");
    assert!(stays_pending(&mut end).await);
    script.release();
    assert_eq!(within(end).await, Ok(()));
}

#[tokio::test]
async fn given_a_never_create_when_it_is_handed_over_then_its_end_never_comes_even_after_a_release()
{
    let (handle, script) = memory_caller();
    script.create(Answer::Never);
    let mut end = handle.create_child(input(1)).expect("create");
    script.release();
    assert!(stays_pending(&mut end).await);
}

#[tokio::test]
async fn given_an_error_scripted_when_the_call_is_made_then_it_fails_at_once_and_the_next_call_is_not_affected()
 {
    let (handle, script) = memory_caller();
    script.upload(Answer::Fail(Error::InvalidInput("scripted".to_owned())));
    assert_eq!(
        handle.upload_payload(id(1), content("p")).err(),
        Some(Error::InvalidInput("scripted".to_owned()))
    );
    assert!(handle.upload_payload(id(1), content("p")).is_ok());
}

#[tokio::test]
async fn given_a_removal_scripted_when_the_end_is_awaited_then_the_removal_comes() {
    let (handle, script) = memory_caller();
    script.finish(Answer::Now(Err(Removal::LeftUnsent)));
    let end = handle.finish(id(1), id(2001)).expect("finish");
    assert_eq!(within(end).await, Err(Removal::LeftUnsent));
}

#[tokio::test]
async fn given_scripted_results_when_they_are_read_then_each_read_takes_the_next_answer() {
    let (handle, script) = memory_caller();
    script.read_result(Answer::Now(content("first")));
    script.read_result(Answer::Fail(Error::Unauthenticated));
    let first = within(handle.read_result(result_ref(id(9)), true)).await;
    assert_eq!(first.expect("a Result"), content("first"));
    let second = within(handle.read_result(result_ref(id(9)), false)).await;
    assert!(second.is_err());
    assert_eq!(
        script.calls(),
        vec![
            MemoryCall::ReadResult {
                result: result_ref(id(9)),
                keep: true
            },
            MemoryCall::ReadResult {
                result: result_ref(id(9)),
                keep: false
            },
        ]
    );
}

#[tokio::test]
async fn given_a_late_read_when_it_is_awaited_then_it_waits_for_the_release() {
    let (handle, script) = memory_caller();
    script.read_result(Answer::Late(content("late")));
    let mut read = Box::pin(handle.read_result(result_ref(id(9)), false));
    assert!(stays_pending(&mut read).await);
    script.release();
    assert_eq!(within(read).await.expect("a Result"), content("late"));
}

#[tokio::test]
async fn given_a_given_end_when_the_wait_is_opened_before_or_after_then_both_give_it() {
    let (handle, script) = memory_caller();
    let before = handle.end_wait(id(5));
    script.end(id(5), Answer::Now(cancelled("stopped")));
    assert_eq!(within(before).await, Ok(cancelled("stopped")));
    assert_eq!(
        within(handle.end_wait(id(5))).await,
        Ok(cancelled("stopped"))
    );
}

#[tokio::test]
async fn given_a_first_end_for_a_child_when_a_second_is_scripted_then_the_first_stays() {
    let (handle, script) = memory_caller();
    script.end(id(5), Answer::Now(cancelled("first")));
    script.end(id(5), Answer::Now(cancelled("second")));
    assert_eq!(within(handle.end_wait(id(5))).await, Ok(cancelled("first")));
}

#[tokio::test]
async fn given_a_late_end_when_the_wait_is_open_then_it_completes_at_the_release() {
    let (handle, script) = memory_caller();
    script.end(id(5), Answer::Late(cancelled("late")));
    let mut wait = handle.end_wait(id(5));
    assert!(stays_pending(&mut wait).await);
    script.release();
    assert_eq!(within(wait).await, Ok(cancelled("late")));
}

#[tokio::test]
async fn given_a_never_end_then_the_wait_stays_pending_and_an_error_end_gives_the_error() {
    let (handle, script) = memory_caller();
    script.end(id(5), Answer::Never);
    assert!(stays_pending(handle.end_wait(id(5))).await);
    script.end(
        id(6),
        Answer::Fail(Error::Unreadable("scripted".to_owned())),
    );
    assert_eq!(
        within(handle.end_wait(id(6))).await,
        Err(Error::Unreadable("scripted".to_owned()))
    );
}

#[tokio::test]
async fn given_the_infra_missing_end_then_open_and_new_waits_give_it_until_the_feed_is_open_again()
{
    let (handle, script) = memory_caller();
    let open = handle.end_wait(id(5));
    script.infra_missing();
    assert!(matches!(within(open).await, Err(Error::InfraMissing(_))));
    assert!(matches!(
        within(handle.end_wait(id(6))).await,
        Err(Error::InfraMissing(_))
    ));
    script.open_again();
    assert!(stays_pending(handle.end_wait(id(7))).await);
}

#[tokio::test]
async fn given_the_book_dropped_then_open_and_new_waits_give_book_dropped_even_for_a_given_end() {
    let (handle, script) = memory_caller();
    script.end(id(5), Answer::Now(cancelled("held")));
    let open = handle.end_wait(id(5));
    let other = tokio::spawn(handle.end_wait(id(6)));
    tokio::task::yield_now().await;
    script.drop_book();
    assert_eq!(within(open).await, Err(Error::BookDropped));
    assert_eq!(
        within(other).await.expect("joined"),
        Err(Error::BookDropped)
    );
    assert_eq!(
        within(handle.end_wait(id(5))).await,
        Err(Error::BookDropped)
    );
}

#[tokio::test]
async fn given_a_cloned_handle_then_it_shares_the_script_and_the_record() {
    let (handle, script) = memory_caller();
    let clone = handle.clone();
    script.end(id(5), Answer::Now(cancelled("shared")));
    assert_eq!(within(clone.end_wait(id(5))).await, Ok(cancelled("shared")));
    assert_eq!(
        script.calls(),
        vec![MemoryCall::EndWait {
            child_job_id: id(5)
        }]
    );
}
