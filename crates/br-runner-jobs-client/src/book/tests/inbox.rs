use std::time::Duration;

use super::*;
use crate::arrival::Arrival;
use crate::book::inbox::Wake;
use crate::book::{EntryKind, MailEntry};
use crate::error::Error;
use crate::model::{CallNoticeKind, ItemArrived, ItemKind};

fn arrived(address: Option<u128>, n: u128) -> ItemArrived {
    ItemArrived {
        address: address.map(id),
        item_id: id(500 + n),
        kind: ItemKind::DirectMail,
    }
}

async fn wake(watch: &mut crate::book::ItemWatch) -> Wake {
    tokio::time::timeout(Duration::from_secs(5), watch.next())
        .await
        .expect("a wake-up")
}

#[tokio::test]
async fn given_a_subscription_when_items_arrive_then_only_the_items_of_its_address_wake_it() {
    let fixture = fixture().await;
    let mut watch = fixture.book.subscribe_items(id(70));
    fixture.book.arrival(&Arrival::Item(arrived(Some(71), 1)));
    fixture.book.arrival(&Arrival::Item(arrived(None, 2)));
    fixture.book.arrival(&Arrival::Item(arrived(Some(70), 3)));
    assert_eq!(wake(&mut watch).await, Wake::Item(arrived(Some(70), 3)));
}

#[tokio::test]
async fn given_two_subscriptions_for_one_address_when_an_item_arrives_then_both_wake() {
    let fixture = fixture().await;
    let mut first = fixture.book.subscribe_items(id(70));
    let mut second = fixture.book.subscribe_items(id(70));
    fixture.book.arrival(&Arrival::Item(arrived(Some(70), 1)));
    assert_eq!(wake(&mut first).await, Wake::Item(arrived(Some(70), 1)));
    assert_eq!(wake(&mut second).await, Wake::Item(arrived(Some(70), 1)));
}

#[tokio::test]
async fn given_a_feed_end_when_it_reaches_the_book_then_open_subscriptions_wake_with_it_for_lost_and_infra_missing_but_not_for_one_unreadable_event()
 {
    let fixture = fixture().await;
    let mut watch = fixture.book.subscribe_items(id(70));
    fixture
        .book
        .arrival_ended(&Error::Unreadable("one event".to_owned()));
    fixture.book.arrival_ended(&Error::lost("cut"));
    assert_eq!(wake(&mut watch).await, Wake::FeedEnded(Error::lost("cut")));
    let infra = Error::InfraMissing("gone".to_owned());
    fixture.book.arrival_ended(&infra);
    assert_eq!(wake(&mut watch).await, Wake::FeedEnded(infra));
}

#[tokio::test]
async fn given_the_lost_end_then_the_book_state_stays_open_and_given_infra_missing_it_is_kept() {
    let fixture = fixture().await;
    fixture.book.arrival_ended(&Error::lost("cut"));
    assert!(fixture.book.arrival_state().is_none());
    fixture
        .book
        .arrival_ended(&Error::InfraMissing("gone".to_owned()));
    assert!(matches!(
        fixture.book.arrival_state(),
        Some(Error::InfraMissing(_))
    ));
    fixture.book.arrival_open_again();
    assert!(fixture.book.arrival_state().is_none());
}

#[tokio::test]
async fn given_a_subscription_when_the_book_is_dropped_then_it_wakes_with_book_dropped() {
    let fixture = fixture().await;
    let mut watch = fixture.book.subscribe_items(id(70));
    drop(fixture.book);
    assert_eq!(wake(&mut watch).await, Wake::BookDropped);
}

#[tokio::test]
async fn given_an_abandoned_subscription_when_items_arrive_then_the_others_still_wake() {
    let fixture = fixture().await;
    let abandoned = fixture.book.subscribe_items(id(70));
    let mut kept = fixture.book.subscribe_items(id(70));
    drop(abandoned);
    fixture.book.arrival(&Arrival::Item(arrived(Some(70), 1)));
    assert_eq!(wake(&mut kept).await, Wake::Item(arrived(Some(70), 1)));
}

#[tokio::test]
async fn given_a_listed_entry_when_it_is_looked_up_then_the_pair_of_agent_and_entry_gives_the_item_and_the_child()
 {
    let fixture = fixture().await;
    let entry = MailEntry {
        item_id: id(501),
        child_job_id: Some(id(300)),
        kind: EntryKind::CallNotice(CallNoticeKind::RunCompleted),
    };
    fixture.book.record_listed(id(70), id(600), entry);
    assert_eq!(fixture.book.listed(id(70), id(600)), Some(entry));
    assert_eq!(fixture.book.listed(id(71), id(600)), None);
    assert_eq!(fixture.book.listed(id(70), id(601)), None);
}

#[tokio::test]
async fn given_a_mail_id_when_it_is_taken_twice_then_only_the_first_take_is_new() {
    let fixture = fixture().await;
    assert!(!fixture.book.mail_taken(id(420)));
    assert!(fixture.book.take_mail(id(420)));
    assert!(!fixture.book.take_mail(id(420)));
    assert!(fixture.book.mail_taken(id(420)));
}

#[tokio::test]
async fn given_marked_items_then_they_are_remembered_for_the_agent_that_marked_them_only() {
    let fixture = fixture().await;
    fixture.book.mark(id(70), id(501));
    assert!(fixture.book.is_marked(id(70), id(501)));
    assert!(!fixture.book.is_marked(id(71), id(501)));
    assert_eq!(fixture.book.marked_items(id(70)).len(), 1);
    assert!(fixture.book.marked_items(id(71)).is_empty());
}

#[test]
fn given_the_handles_the_book_and_the_waits_then_they_can_cross_threads() {
    fn send<T: Send>() {}
    fn send_sync<T: Send + Sync>() {}
    send_sync::<crate::book::AgentBook>();
    send_sync::<crate::caller::CallerHandle>();
    send_sync::<crate::mail::MailHandle>();
    send::<crate::endwait::EndWait>();
    send::<crate::arrival::ArrivalFeed>();
}
