use super::*;

fn bad_mail(n: u128) -> Value {
    let mut item = mail(n);
    item["author"] = Value::Null;
    item
}

fn bad_completion(n: u128) -> Value {
    notice(n, "RUN_COMPLETED", json!({ "resultId": "nope" }))
}

#[tokio::test]
async fn given_a_bad_item_among_good_ones_in_the_replay_then_the_good_ones_pass_and_the_bad_one_comes_alone()
 {
    let jobs = scripted();
    let (mut feed, _pusher) = open(
        &jobs,
        vec![
            mail(1),
            bad_mail(2),
            notice(3, "RUN_COMPLETED", json!({ "resultId": id(800) })),
        ],
    )
    .await;
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(501)));
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::Unreadable {
            child_job_id: None,
            error: Error::Unreadable(_)
        })
    ));
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(503)));
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::SyncEnd {
            child_job_id,
            end: SyncEnd::Completed { .. }
        }) if child_job_id == id(703)
    ));
}

#[tokio::test]
async fn given_a_bad_notice_of_one_child_in_the_replay_then_only_that_child_is_named_and_the_other_ends()
 {
    let jobs = scripted();
    let (mut feed, _pusher) = open(
        &jobs,
        vec![
            bad_completion(1),
            notice(2, "JOB_FAILED", json!({ "message": "It failed." })),
        ],
    )
    .await;
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::Unreadable {
            child_job_id: Some(child),
            ..
        }) if child == id(701)
    ));
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(502)));
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::SyncEnd {
            child_job_id,
            end: SyncEnd::Failed { .. }
        }) if child_job_id == id(702)
    ));
}

#[tokio::test]
async fn given_a_live_notice_beside_a_bad_item_of_another_child_then_the_live_notice_still_ends_its_child()
 {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, Vec::new()).await;
    jobs.push_graphql(Ok(unread(vec![
        bad_completion(3),
        notice(4, "RUN_COMPLETED", json!({ "resultId": id(800) })),
    ])));
    pusher
        .send(Ok(arrived(None, 4, ItemKind::CallNotice)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(504)));
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::SyncEnd { child_job_id, .. }) if child_job_id == id(704)
    ));
    pusher
        .send(Ok(arrived(Some(id(70)), 5, ItemKind::DirectMail)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(505)));
}

#[tokio::test]
async fn given_a_live_notice_that_is_bad_then_only_the_wait_of_its_child_ends_with_unreadable() {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, Vec::new()).await;
    jobs.push_graphql(Ok(unread(vec![
        bad_completion(4),
        notice(5, "RUN_COMPLETED", json!({ "resultId": id(800) })),
    ])));
    pusher
        .send(Ok(arrived(None, 4, ItemKind::CallNotice)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(504)));
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::Unreadable {
            child_job_id: Some(child),
            ..
        }) if child == id(704)
    ));
    pusher
        .send(Ok(arrived(Some(id(70)), 6, ItemKind::DirectMail)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(506)));
}

#[tokio::test]
async fn given_a_run_level_notice_of_an_unknown_type_then_it_arrives_as_an_item_and_ends_nothing() {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, vec![notice(1, "PAUSED", json!({}))]).await;
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::Item(item)) if item.item_id == id(501) && item.kind == ItemKind::CallNotice
    ));
    pusher
        .send(Ok(arrived(Some(id(70)), 2, ItemKind::DirectMail)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(502)));
}

#[tokio::test]
async fn given_an_item_of_an_unknown_kind_in_the_replay_then_it_arrives_as_an_unknown_item() {
    let jobs = scripted();
    let mut item = mail(1);
    item["itemKind"] = json!("ELSE");
    let (mut feed, _pusher) = open(&jobs, vec![item]).await;
    assert!(matches!(
        next(&mut feed).await,
        Ok(Arrival::Item(item)) if item.item_id == id(501) && item.kind == ItemKind::Unknown
    ));
}
