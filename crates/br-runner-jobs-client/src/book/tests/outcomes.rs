use std::time::Duration;

use super::*;
use crate::arrival::Arrival;
use crate::error::Error;
use crate::model::{ResultRef, SyncEnd};

pub(super) fn cancelled(child: u128, message: &str) -> Arrival {
    Arrival::SyncEnd {
        child_job_id: id(child),
        end: SyncEnd::Cancelled {
            message: message.to_owned(),
        },
    }
}

pub(super) fn completed(child: u128, result: u128) -> Arrival {
    Arrival::SyncEnd {
        child_job_id: id(child),
        end: SyncEnd::Completed {
            inline: None,
            result_ref: Some(ResultRef(id(result))),
        },
    }
}

pub(super) fn unreadable(child: Option<u128>) -> Arrival {
    Arrival::Unreadable {
        child_job_id: child.map(id),
        error: Error::Unreadable("a broken end".to_owned()),
    }
}

pub(super) async fn pending(wait: &mut crate::endwait::EndWait) -> bool {
    tokio::time::timeout(Duration::from_millis(50), wait)
        .await
        .is_err()
}

#[tokio::test]
async fn given_an_end_held_by_the_book_when_a_wait_is_opened_then_it_gives_the_end_at_once_every_time()
 {
    let fixture = fixture().await;
    fixture.book.arrival(&cancelled(10, "stopped"));
    let handle = fixture.book.caller_handle();
    for _ in 0..3 {
        let end = handle.end_wait(id(10)).await;
        assert_eq!(
            end,
            Ok(SyncEnd::Cancelled {
                message: "stopped".to_owned()
            })
        );
    }
}

#[tokio::test]
async fn given_two_ends_for_one_child_when_both_reach_the_book_then_the_first_stays_also_after_a_replay()
 {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    fixture.book.arrival(&completed(10, 50));
    fixture.book.arrival(&cancelled(10, "later"));
    fixture.book.arrival(&completed(10, 51));
    let expected = Ok(SyncEnd::Completed {
        inline: None,
        result_ref: Some(ResultRef(id(50))),
    });
    assert_eq!(handle.end_wait(id(10)).await, expected);
}

#[tokio::test]
async fn given_an_unreadable_with_a_child_id_when_a_later_end_arrives_then_the_unreadable_stays_the_outcome()
 {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    fixture.book.arrival(&unreadable(Some(10)));
    fixture.book.arrival(&completed(10, 50));
    assert!(matches!(
        handle.end_wait(id(10)).await,
        Err(Error::Unreadable(_))
    ));
}

#[tokio::test]
async fn given_an_end_after_an_unreadable_for_another_child_then_each_child_has_its_own_outcome() {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    fixture.book.arrival(&unreadable(Some(10)));
    fixture.book.arrival(&cancelled(11, "other"));
    assert!(matches!(
        handle.end_wait(id(10)).await,
        Err(Error::Unreadable(_))
    ));
    assert!(matches!(
        handle.end_wait(id(11)).await,
        Ok(SyncEnd::Cancelled { .. })
    ));
}

#[tokio::test]
async fn given_an_unreadable_without_a_child_id_when_it_reaches_the_book_then_no_wait_ends() {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    let mut wait = handle.end_wait(id(10));
    fixture.book.arrival(&unreadable(None));
    assert!(pending(&mut wait).await);
}

#[tokio::test]
async fn given_an_item_event_when_it_reaches_the_book_then_no_end_wait_ends() {
    let fixture = fixture().await;
    let handle = fixture.book.caller_handle();
    let mut wait = handle.end_wait(id(10));
    fixture
        .book
        .arrival(&Arrival::Item(crate::model::ItemArrived {
            address: Some(id(60)),
            item_id: id(61),
            kind: crate::model::ItemKind::DirectMail,
        }));
    assert!(pending(&mut wait).await);
}
