use super::*;
use crate::error::Error;
use crate::model::{CancelEvent, DispatchMode, ModeEvent};

#[tokio::test]
async fn given_a_cancel_feed_when_events_come_then_each_is_given_in_order() {
    let (jobs, client) = scripted();
    let mut feed = client.cancel_orders().await.expect("a feed");
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(2) }));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
    jobs.push_cancel(Ok(CancelEvent::OrderDeleted { run_id: id(2) }));
    assert_eq!(
        feed.next().await,
        Ok(CancelEvent::OrderMade { run_id: id(2) })
    );
    assert_eq!(feed.next().await, Ok(CancelEvent::ReplayComplete));
    assert_eq!(
        feed.next().await,
        Ok(CancelEvent::OrderDeleted { run_id: id(2) })
    );
}

#[tokio::test]
async fn given_an_event_that_cannot_be_decoded_when_the_feed_is_read_then_it_is_an_error_event_and_the_feed_goes_on()
 {
    let (jobs, client) = scripted();
    let mut feed = client.dispatch_modes().await.expect("a feed");
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Push)));
    jobs.push_mode(Err(Error::Unreadable("an entry".to_owned())));
    jobs.push_mode(Err(Error::Unreadable("another entry".to_owned())));
    jobs.push_mode(Ok(ModeEvent::NoEntry));
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Claim)));
    assert_eq!(feed.next().await, Ok(ModeEvent::Mode(DispatchMode::Push)));
    assert!(matches!(feed.next().await, Err(Error::Unreadable(_))));
    assert!(matches!(feed.next().await, Err(Error::Unreadable(_))));
    assert_eq!(feed.next().await, Ok(ModeEvent::NoEntry));
    assert_eq!(feed.next().await, Ok(ModeEvent::Mode(DispatchMode::Claim)));
}

#[tokio::test]
async fn given_any_other_error_when_the_feed_is_read_then_the_feed_ends_and_the_error_is_repeated_without_reading_the_source()
 {
    for ended in [
        Error::lost("the watch ended"),
        Error::InfraMissing("the bucket is gone".to_owned()),
        Error::Unauthenticated,
        Error::unavailable("no service"),
    ] {
        let (jobs, client) = scripted();
        let mut feed = client.cancel_orders().await.expect("a feed");
        jobs.push_cancel(Err(ended.clone()));
        jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
        assert_eq!(feed.next().await, Err(ended.clone()));
        assert_eq!(feed.next().await, Err(ended.clone()));
        assert_eq!(feed.next().await, Err(ended));
    }
}

#[tokio::test]
async fn given_an_ended_feed_when_it_is_shown_then_the_debug_form_says_it_ended() {
    let (jobs, client) = scripted();
    let mut feed = client.dispatch_modes().await.expect("a feed");
    assert!(format!("{feed:?}").contains("false"));
    jobs.push_mode(Err(Error::lost("cut")));
    let _ = feed.next().await;
    assert!(format!("{feed:?}").contains("true"));
}

#[tokio::test]
async fn given_the_mode_feed_when_a_no_entry_event_comes_then_it_is_never_turned_into_a_mode() {
    let (jobs, client) = scripted();
    let mut feed = client.dispatch_modes().await.expect("a feed");
    jobs.push_mode(Ok(ModeEvent::NoEntry));
    assert_eq!(feed.next().await, Ok(ModeEvent::NoEntry));
}
