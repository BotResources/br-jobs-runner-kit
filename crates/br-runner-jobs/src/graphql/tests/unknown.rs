use serde_json::json;

use super::unread::{mail_item, notice_item, reply};
use super::{id, ids, jobs};
use crate::graphql;
use crate::model::{CallNoticeKind, UnreadItem};

#[tokio::test]
async fn given_an_item_of_an_unknown_kind_then_unread_keeps_it_as_unknown_with_the_kind_of_jobs() {
    let mut item = mail_item(1);
    item["itemKind"] = json!("ELSE");
    item["message"] = json!("Something new.");
    let scripted = jobs(reply(vec![item]));
    let items = graphql::unread(&*scripted, &ids(), None).await.unwrap();
    assert_eq!(
        items,
        vec![UnreadItem::Unknown {
            item_id: id(101),
            address: Some(id(4)),
            item_kind: "ELSE".to_owned(),
            message: Some("Something new.".to_owned()),
        }]
    );
}

#[tokio::test]
async fn given_a_notice_of_an_unknown_type_then_unread_keeps_it_as_an_unknown_notice_with_its_type()
{
    let scripted = jobs(reply(vec![notice_item(1, "CANCELLED")]));
    let items = graphql::unread(&*scripted, &ids(), None).await.unwrap();
    let [UnreadItem::CallNotice(notice)] = items.as_slice() else {
        panic!("{items:?}");
    };
    assert_eq!(notice.kind, CallNoticeKind::Unknown);
    assert_eq!(notice.notice_type.as_deref(), Some("CANCELLED"));
    assert_eq!(notice.child_job_id, Some(id(301)));
}

#[tokio::test]
async fn given_a_notice_of_an_unknown_type_without_a_child_job_then_it_is_kept_without_one() {
    let mut item = notice_item(1, "CANCELLED");
    item.as_object_mut().unwrap().remove("jobId");
    let scripted = jobs(reply(vec![item]));
    let items = graphql::unread(&*scripted, &ids(), None).await.unwrap();
    let [UnreadItem::CallNotice(notice)] = items.as_slice() else {
        panic!("{items:?}");
    };
    assert_eq!(notice.child_job_id, None);
}

#[tokio::test]
async fn given_a_notice_of_a_known_type_with_a_known_type_name_then_no_type_name_is_kept() {
    let scripted = jobs(reply(vec![notice_item(1, "ACCEPTED")]));
    let items = graphql::unread(&*scripted, &ids(), None).await.unwrap();
    let [UnreadItem::CallNotice(notice)] = items.as_slice() else {
        panic!("{items:?}");
    };
    assert_eq!(notice.notice_type, None);
}
