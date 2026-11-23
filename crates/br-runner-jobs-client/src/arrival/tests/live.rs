use super::*;
use crate::arrival::settings_of_tests;

#[tokio::test]
async fn given_a_live_run_level_notice_when_it_arrives_then_one_read_gives_the_item_then_its_sync_end()
 {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, Vec::new()).await;
    jobs.push_graphql(Ok(unread(vec![
        mail(1),
        notice(
            4,
            "RUN_COMPLETED",
            json!({ "answer": { "message": "done", "attachments": [] } }),
        ),
    ])));
    pusher
        .send(Ok(arrived(None, 4, ItemKind::CallNotice)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(504)));
    let Ok(Arrival::SyncEnd {
        child_job_id,
        end: SyncEnd::Completed { inline, .. },
    }) = next(&mut feed).await
    else {
        panic!("a completion is expected");
    };
    assert_eq!(child_job_id, id(704));
    assert_eq!(inline.expect("inline").message, "done");
    assert_eq!(reads(&jobs), 2);
}

#[tokio::test]
async fn given_live_events_for_an_address_when_they_arrive_then_no_read_is_made() {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, Vec::new()).await;
    pusher
        .send(Ok(arrived(Some(id(70)), 1, ItemKind::DirectMail)))
        .expect("sent");
    pusher
        .send(Ok(arrived(Some(id(70)), 2, ItemKind::CcNotice)))
        .expect("sent");
    let _ = next(&mut feed).await;
    let _ = next(&mut feed).await;
    assert_eq!(reads(&jobs), 1);
}

#[tokio::test]
async fn given_an_item_that_is_replayed_and_arrives_live_then_it_comes_twice_because_the_importer_dedupes()
 {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, vec![mail(1)]).await;
    pusher
        .send(Ok(arrived(Some(id(70)), 1, ItemKind::DirectMail)))
        .expect("sent");
    let first = next(&mut feed).await.expect("first");
    let second = next(&mut feed).await.expect("second");
    assert_eq!(first, second);
}

#[tokio::test]
async fn given_a_live_run_level_notice_whose_item_is_missing_from_the_read_then_only_the_item_comes_and_the_feed_goes_on()
 {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, Vec::new()).await;
    jobs.push_graphql(Ok(unread(Vec::new())));
    pusher
        .send(Ok(arrived(None, 4, ItemKind::CallNotice)))
        .expect("sent");
    pusher
        .send(Ok(arrived(Some(id(70)), 5, ItemKind::DirectMail)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(504)));
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(505)));
}

#[tokio::test]
async fn given_a_completion_without_result_or_a_failure_without_message_then_the_event_is_unreadable_and_names_the_child()
 {
    let jobs = scripted();
    let (mut feed, _pusher) = open(
        &jobs,
        vec![
            notice(1, "RUN_COMPLETED", json!({})),
            notice(2, "JOB_FAILED", json!({})),
        ],
    )
    .await;
    let mut named = Vec::new();
    for _ in 0..4 {
        if let Ok(Arrival::Unreadable {
            child_job_id,
            error,
        }) = next(&mut feed).await
        {
            assert!(matches!(error, Error::Unreadable(_)));
            named.push(child_job_id);
        }
    }
    assert_eq!(named, vec![Some(id(701)), Some(id(702))]);
}

#[tokio::test]
async fn given_an_unreadable_raw_event_when_it_arrives_then_it_is_an_event_and_the_feed_goes_on() {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, Vec::new()).await;
    pusher
        .send(Err(Error::Unreadable("one event".to_owned())))
        .expect("sent");
    pusher
        .send(Ok(arrived(Some(id(70)), 1, ItemKind::DirectMail)))
        .expect("sent");
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::Unreadable {
            child_job_id: None,
            ..
        })
    ));
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(_))));
}

#[tokio::test]
async fn given_a_replay_that_cannot_be_read_when_the_feed_is_opened_then_an_unreadable_event_comes_and_live_events_follow()
 {
    let jobs = scripted();
    jobs.push_graphql(Ok(json!({ "other": {} })));
    let work = work(&jobs);
    let (pusher, receiver) = mpsc::unbounded_channel();
    let mut feed = ArrivalFeed::from_raw(Arc::clone(work.core()), Box::new(Channel(receiver)))
        .await
        .expect("a feed");
    pusher
        .send(Ok(arrived(Some(id(70)), 1, ItemKind::DirectMail)))
        .expect("sent");
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::Unreadable {
            child_job_id: None,
            ..
        })
    ));
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(_))));
}

#[tokio::test]
async fn given_a_replay_read_that_is_lost_when_the_feed_is_opened_then_the_opening_fails_with_it() {
    let jobs = scripted();
    jobs.push_graphql(Err(Error::lost("cut")));
    let work = work(&jobs);
    let (_pusher, receiver) = mpsc::unbounded_channel();
    let opened = ArrivalFeed::from_raw(Arc::clone(work.core()), Box::new(Channel(receiver))).await;
    assert_eq!(opened.err(), Some(Error::lost("cut")));
}

#[tokio::test]
async fn given_the_raw_feed_ends_when_the_buffer_is_drained_then_the_typed_error_comes_and_keeps_coming()
 {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, vec![mail(1)]).await;
    pusher.send(Err(Error::lost("cut"))).expect("sent");
    pusher
        .send(Ok(arrived(Some(id(70)), 2, ItemKind::DirectMail)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(_))));
    assert_eq!(next(&mut feed).await, Err(Error::lost("cut")));
    assert_eq!(next(&mut feed).await, Err(Error::lost("cut")));
}

#[tokio::test]
async fn given_the_stream_is_gone_when_the_raw_feed_ends_then_infra_missing_ends_the_feed() {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, Vec::new()).await;
    pusher
        .send(Err(Error::InfraMissing("the stream".to_owned())))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Err(Error::InfraMissing(_))));
}

#[tokio::test]
async fn given_a_resolving_read_that_fails_then_the_item_comes_first_and_the_feed_ends_with_the_error()
 {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, Vec::new()).await;
    jobs.push_graphql(Err(Error::lost("cut")));
    pusher
        .send(Ok(arrived(None, 4, ItemKind::CallNotice)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(_))));
    assert_eq!(next(&mut feed).await, Err(Error::lost("cut")));
    assert_eq!(next(&mut feed).await, Err(Error::lost("cut")));
}

#[tokio::test]
async fn given_a_next_dropped_during_the_resolving_read_when_next_is_called_again_then_the_sync_end_is_not_lost()
 {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, Vec::new()).await;
    jobs.push_graphql(Ok(unread(vec![notice(
        4,
        "RUN_COMPLETED",
        json!({ "resultId": id(800) }),
    )])));
    pusher
        .send(Ok(arrived(None, 4, ItemKind::CallNotice)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(_))));
    jobs.pause();
    {
        let mut waiting = Box::pin(feed.next());
        assert!(poll!(waiting.as_mut()).is_pending());
    }
    jobs.resume();
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::SyncEnd { child_job_id, .. }) if child_job_id == id(704)
    ));
}

#[tokio::test]
async fn given_a_work_when_the_arrival_feed_is_opened_then_the_raw_feed_of_the_run_gives_the_live_events()
 {
    let jobs = scripted();
    jobs.push_graphql(Ok(unread(vec![mail(1)])));
    let work = work(&jobs);
    let mut feed = work
        .arrival_feed(settings_of_tests())
        .await
        .expect("a feed");
    jobs.push_arrival(Ok(arrived(Some(id(70)), 2, ItemKind::CcNotice)));
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(501)));
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(502)));
}

#[tokio::test]
async fn given_the_importer_feeds_the_book_when_a_sync_end_comes_then_the_end_wait_completes() {
    let fixture = fixture().await;
    fixture.queue.drained().await;
    fixture.jobs.push_graphql(Ok(unread(vec![notice(
        1,
        "RUN_COMPLETED",
        json!({ "resultId": id(800) }),
    )])));
    let mut feed = fixture
        .work
        .arrival_feed(settings_of_tests())
        .await
        .expect("a feed");
    let wait = tokio::spawn(fixture.book.caller_handle().end_wait(id(701)));
    for _ in 0..2 {
        let arrival = next(&mut feed).await.expect("an arrival");
        fixture.book.arrival(&arrival);
    }
    assert!(matches!(
        wait.await.expect("joined"),
        Ok(SyncEnd::Completed { .. })
    ));
}
