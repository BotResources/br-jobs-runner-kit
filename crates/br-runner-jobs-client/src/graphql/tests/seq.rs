use serde_json::{Value, json};

use super::unread::{mail_item, notice_item, reply};
use super::{ids, jobs};
use crate::error::Error;
use crate::graphql;
use crate::model::UnreadItem;

fn with_kind(kind: &str) -> Value {
    let mut item = mail_item(1);
    item["itemKind"] = json!(kind);
    item["noticeType"] = json!("STARTED");
    item["jobId"] = notice_item(1, "STARTED")["jobId"].clone();
    item["message"] = json!("m");
    item["seq"] = json!(-7);
    item
}

fn with_seq(seq: Value) -> Value {
    let mut item = mail_item(1);
    item["seq"] = seq;
    item
}

#[tokio::test]
async fn given_an_item_of_each_kind_when_read_with_the_seq_then_it_is_the_integer_of_the_wire() {
    let wire = ["MAIL", "CC_NOTICE", "NOTICE", "MAIL_NOTICE", "OTHER"]
        .map(with_kind)
        .to_vec();
    let scripted = jobs(reply(wire));
    let items = graphql::unread_sequenced(&*scripted, &ids(), None)
        .await
        .unwrap();
    assert_eq!(items.len(), 5);
    assert!(items.iter().all(|found| found.seq == -7), "{items:?}");
    assert!(matches!(items[2].item, UnreadItem::CallNotice(_)));
    assert!(matches!(items[4].item, UnreadItem::Unknown { .. }));
}

#[tokio::test]
async fn given_a_seq_that_is_missing_null_or_not_an_integer_when_read_with_the_seq_then_it_is_unreadable()
 {
    let mut missing = mail_item(1);
    missing.as_object_mut().unwrap().remove("seq");
    for bad in [
        missing,
        with_seq(Value::Null),
        with_seq(json!("1")),
        with_seq(json!(1.5)),
        with_seq(json!(u64::MAX)),
    ] {
        let scripted = jobs(reply(vec![bad]));
        let error = graphql::unread_sequenced(&*scripted, &ids(), None)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
    }
}

#[tokio::test]
async fn given_a_missing_seq_when_the_plain_unread_is_read_then_it_still_reads_as_before() {
    let mut missing = mail_item(1);
    missing.as_object_mut().unwrap().remove("seq");
    let scripted = jobs(reply(vec![missing]));
    assert_eq!(
        graphql::unread(&*scripted, &ids(), None)
            .await
            .unwrap()
            .len(),
        1
    );
}
