use serde_json::{Value, json};

use super::unread::{mail_item, notice_item, reply};
use super::unreadable::assert_unreadable;
use super::{id, ids, jobs};
use crate::graphql::{self, Read};

#[tokio::test]
async fn given_one_bad_item_among_good_ones_then_the_item_level_read_gives_each_alone_in_order() {
    let mut bad = mail_item(2);
    bad["author"] = Value::Null;
    let scripted = jobs(reply(vec![mail_item(1), bad, mail_item(3)]));
    let reads = graphql::unread_each(&*scripted, &ids(), None)
        .await
        .unwrap();
    let [Read::Item(first), Read::Bad(bad), Read::Item(last)] = reads.as_slice() else {
        panic!("{reads:?}");
    };
    assert_eq!((first.item_id(), last.item_id()), (id(101), id(103)));
    assert_eq!(bad.item_id, Some(id(102)));
    assert_eq!(bad.child_job_id, None);
    assert_unreadable(&bad.error);
}

#[tokio::test]
async fn given_a_bad_notice_when_read_item_by_item_then_it_names_its_child_job_and_its_item() {
    let mut bad = notice_item(1, "RUN_COMPLETED");
    bad["resultId"] = json!("nope");
    let scripted = jobs(reply(vec![bad]));
    let reads = graphql::unread_each(&*scripted, &ids(), None)
        .await
        .unwrap();
    let [Read::Bad(bad)] = reads.as_slice() else {
        panic!("{reads:?}");
    };
    assert_eq!(
        (bad.item_id, bad.child_job_id),
        (Some(id(101)), Some(id(301)))
    );
}

#[tokio::test]
async fn given_an_item_without_its_id_when_read_item_by_item_then_it_is_bad_with_no_item_id() {
    let mut bad = mail_item(1);
    bad["itemId"] = Value::Null;
    let scripted = jobs(reply(vec![bad]));
    let reads = graphql::unread_each(&*scripted, &ids(), None)
        .await
        .unwrap();
    let [Read::Bad(bad)] = reads.as_slice() else {
        panic!("{reads:?}");
    };
    assert_eq!(bad.item_id, None);
}
