use serde_json::json;

use super::{Arrival, ItemKind, SyncEnd, arrived, id, next, notice, open, scripted, unread};

#[tokio::test]
async fn given_a_run_level_cancel_in_the_replay_then_the_message_comes_with_the_cancelled_sync_end()
{
    let jobs = scripted();
    let (mut feed, _pusher) = open(
        &jobs,
        vec![notice(
            1,
            "JOB_CANCELLED",
            json!({ "message": "Job was cancelled." }),
        )],
    )
    .await;
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(_))));
    assert_eq!(
        next(&mut feed).await,
        Ok(Arrival::SyncEnd {
            child_job_id: id(701),
            end: SyncEnd::Cancelled {
                message: "Job was cancelled.".to_owned()
            },
        })
    );
}

#[tokio::test]
async fn given_a_live_run_level_cancel_when_the_feed_reads_the_run_then_the_cancelled_sync_end_comes()
 {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, Vec::new()).await;
    jobs.push_graphql(Ok(unread(vec![notice(
        4,
        "JOB_CANCELLED",
        json!({ "message": "Job was cancelled." }),
    )])));
    pusher
        .send(Ok(arrived(None, 4, ItemKind::CallNotice)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(504)));
    assert_eq!(
        next(&mut feed).await,
        Ok(Arrival::SyncEnd {
            child_job_id: id(704),
            end: SyncEnd::Cancelled {
                message: "Job was cancelled.".to_owned()
            },
        })
    );
}

#[tokio::test]
async fn given_a_run_level_cancel_without_a_message_then_the_event_is_unreadable_and_names_the_child()
 {
    let jobs = scripted();
    let (mut feed, _pusher) = open(&jobs, vec![notice(1, "JOB_CANCELLED", json!({}))]).await;
    let _ = next(&mut feed).await;
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::Unreadable {
            child_job_id: Some(child),
            ..
        }) if child == id(701)
    ));
}
