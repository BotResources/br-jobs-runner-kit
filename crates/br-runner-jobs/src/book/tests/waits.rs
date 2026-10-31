use std::time::Duration;

use super::outcomes::{cancelled, completed, pending};
use super::*;
use crate::arrival::Arrival;
use crate::error::Error;
use crate::model::SyncEnd;

fn infra() -> Error {
    Error::InfraMissing("the stream is gone".to_owned())
}

async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("the wait ended")
}

#[tokio::test]
async fn given_a_wait_opened_before_its_end_when_the_end_arrives_then_the_wait_completes() {
    let fixture = fixture().await;
    let wait = tokio::spawn(fixture.book.caller_handle().end_wait(id(10)));
    tokio::task::yield_now().await;
    fixture.book.arrival(&cancelled(10, "stopped"));
    assert!(matches!(
        within(wait).await.expect("joined"),
        Ok(SyncEnd::Cancelled { .. })
    ));
}

#[tokio::test]
async fn given_a_wait_opened_before_any_create_when_it_is_opened_then_jobs_receives_nothing() {
    let fixture = fixture().await;
    fixture.queue.drained().await;
    let before = fixture.jobs.calls().len();
    let mut wait = fixture.book.caller_handle().end_wait(id(10));
    assert!(pending(&mut wait).await);
    assert_eq!(fixture.jobs.calls().len(), before);
}

#[tokio::test]
async fn given_an_abandoned_wait_when_other_ends_arrive_then_no_other_wait_loses_its_end() {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    let abandoned = handle.end_wait(id(10));
    let kept = tokio::spawn(handle.end_wait(id(11)));
    tokio::task::yield_now().await;
    drop(abandoned);
    fixture.book.arrival(&cancelled(10, "nobody waits"));
    fixture.book.arrival(&cancelled(11, "kept"));
    assert!(matches!(
        within(kept).await.expect("joined"),
        Ok(SyncEnd::Cancelled { .. })
    ));
    assert!(matches!(
        handle.end_wait(id(10)).await,
        Ok(SyncEnd::Cancelled { .. })
    ));
}

#[tokio::test]
async fn given_the_infra_missing_end_when_it_reaches_the_book_then_every_open_wait_gives_it_and_a_new_wait_gives_it_at_once()
 {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    let open = tokio::spawn(handle.end_wait(id(10)));
    tokio::task::yield_now().await;
    fixture.book.arrival_ended(&infra());
    assert!(matches!(
        within(open).await.expect("joined"),
        Err(Error::InfraMissing(_))
    ));
    assert!(matches!(
        within(handle.end_wait(id(11))).await,
        Err(Error::InfraMissing(_))
    ));
    assert!(fixture.book.arrival_state().is_some());
}

#[tokio::test]
async fn given_the_kept_infra_missing_end_when_the_feed_is_open_again_then_a_new_wait_stays_pending()
 {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    fixture.book.arrival_ended(&infra());
    fixture.book.arrival_open_again();
    assert!(fixture.book.arrival_state().is_none());
    let mut wait = handle.end_wait(id(10));
    assert!(pending(&mut wait).await);
    fixture.book.arrival(&cancelled(10, "back"));
    assert!(matches!(within(wait).await, Ok(SyncEnd::Cancelled { .. })));
}

#[tokio::test]
async fn given_an_open_wait_not_yet_polled_when_the_end_and_the_open_again_come_in_a_row_then_the_wait_still_gives_infra_missing()
 {
    let fixture = fixture().await;
    let wait = fixture.book.caller_handle().end_wait(id(10));
    fixture.book.arrival_ended(&infra());
    fixture.book.arrival_open_again();
    assert!(matches!(within(wait).await, Err(Error::InfraMissing(_))));
}

#[tokio::test]
async fn given_an_outcome_and_the_infra_missing_end_when_waits_are_opened_then_the_outcome_wins_for_its_child()
 {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    fixture.book.arrival(&completed(10, 50));
    fixture.book.arrival_ended(&infra());
    assert!(matches!(
        within(handle.end_wait(id(10))).await,
        Ok(SyncEnd::Completed { .. })
    ));
    assert!(matches!(
        within(handle.end_wait(id(11))).await,
        Err(Error::InfraMissing(_))
    ));
}

#[tokio::test]
async fn given_the_lost_end_when_it_reaches_the_book_then_no_wait_ends_old_or_new_until_an_end_comes()
 {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    let mut old = handle.end_wait(id(10));
    fixture.book.arrival_ended(&Error::lost("cut"));
    let mut new = handle.end_wait(id(11));
    assert!(pending(&mut old).await);
    assert!(pending(&mut new).await);
    assert!(fixture.book.arrival_state().is_none());
    fixture.book.arrival(&cancelled(10, "after the reopening"));
    assert!(matches!(within(old).await, Ok(SyncEnd::Cancelled { .. })));
}

#[tokio::test]
async fn given_an_error_that_is_not_a_feed_end_when_it_reaches_the_book_then_no_wait_ends() {
    let fixture = fixture().await;
    let mut wait = fixture.book.caller_handle().end_wait(id(10));
    fixture
        .book
        .arrival_ended(&Error::Unreadable("one event".to_owned()));
    fixture.book.arrival_ended(&Error::Unauthenticated);
    assert!(pending(&mut wait).await);
}

#[tokio::test]
async fn given_open_waits_when_the_book_is_dropped_then_each_gives_book_dropped() {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    let first = tokio::spawn(handle.end_wait(id(10)));
    let second = tokio::spawn(handle.end_wait(id(11)));
    tokio::task::yield_now().await;
    drop(fixture.book);
    assert_eq!(
        within(first).await.expect("joined"),
        Err(Error::BookDropped)
    );
    assert_eq!(
        within(second).await.expect("joined"),
        Err(Error::BookDropped)
    );
}

#[tokio::test]
async fn given_a_dropped_book_when_a_wait_is_opened_on_its_handle_then_it_gives_book_dropped_even_for_a_held_end()
 {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    fixture.book.arrival(&cancelled(10, "held"));
    let before = handle.end_wait(id(10));
    drop(fixture.book);
    assert_eq!(within(before).await, Err(Error::BookDropped));
    assert_eq!(
        within(handle.end_wait(id(10))).await,
        Err(Error::BookDropped)
    );
}

#[tokio::test]
async fn given_a_dropped_book_when_the_handle_hands_over_then_the_writes_still_go_to_the_queue() {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    drop(fixture.book);
    let end = handle
        .upload_payload(id(300), content("payload"))
        .expect("handed over");
    assert_eq!(end.await, Ok(()));
    let uploads = fixture
        .jobs
        .calls()
        .into_iter()
        .filter(|call| matches!(call, crate::test_double::Call::Graphql { .. }))
        .count();
    assert_eq!(uploads, 1);
}

#[tokio::test]
async fn given_the_arrival_replayed_after_a_reopening_when_the_same_end_comes_again_then_the_outcome_is_unchanged()
 {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    fixture.book.arrival_ended(&infra());
    fixture.book.arrival_open_again();
    let replay = completed(10, 50);
    fixture.book.arrival(&replay);
    fixture.book.arrival(&replay);
    fixture.book.arrival(&Arrival::SyncEnd {
        child_job_id: id(10),
        end: SyncEnd::Cancelled {
            message: "changed".to_owned(),
        },
    });
    assert!(matches!(
        handle.end_wait(id(10)).await,
        Ok(SyncEnd::Completed { .. })
    ));
}
