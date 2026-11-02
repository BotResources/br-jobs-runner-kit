use serde_json::{Value, json};

use super::unread::{notice_item, reply};
use super::{id, ids, jobs};
use crate::error::Error;
use crate::graphql;
use crate::model::{CallNoticeKind, UnreadItem};

fn mail_notice_item(n: u128) -> Value {
    json!({
        "itemId": id(100 + n), "seq": n, "itemKind": "MAIL_NOTICE", "address": id(4),
        "runId": id(2), "mailId": id(200 + n), "noticeType": "REFUSED",
        "message": "At least one recipient cannot receive the mail.",
    })
}

async fn read(item: Value) -> Result<Vec<UnreadItem>, Error> {
    let scripted = jobs(reply(vec![item]));
    graphql::unread(&*scripted, &ids(), None).await
}

#[tokio::test]
async fn given_a_mail_notice_when_unread_is_read_then_the_text_the_mail_and_the_address_are_given()
{
    let items = read(mail_notice_item(1)).await.unwrap();
    assert_eq!(
        items,
        vec![UnreadItem::MailNotice {
            item_id: id(101),
            address: Some(id(4)),
            mail_id: Some(id(201)),
            text: "At least one recipient cannot receive the mail.".to_owned(),
        }]
    );
}

#[tokio::test]
async fn given_a_mail_notice_without_a_mail_id_when_unread_is_read_then_it_is_kept_without_one() {
    let mut item = mail_notice_item(1);
    item.as_object_mut().unwrap().remove("mailId");
    let items = read(item).await.unwrap();
    assert!(matches!(
        items.as_slice(),
        [UnreadItem::MailNotice { mail_id: None, .. }]
    ));
}

#[tokio::test]
async fn given_a_mail_notice_without_a_text_when_unread_is_read_then_it_is_unreadable() {
    let mut item = mail_notice_item(1);
    item.as_object_mut().unwrap().remove("message");
    assert!(matches!(
        read(item).await.unwrap_err(),
        Error::Unreadable(_)
    ));
}

#[tokio::test]
async fn given_a_cancel_notice_when_unread_is_read_then_it_is_a_cancel_with_its_child_job_and_message()
 {
    let mut item = notice_item(1, "JOB_CANCELLED");
    item["message"] = json!("Job was cancelled.");
    let items = read(item).await.unwrap();
    let [UnreadItem::CallNotice(notice)] = items.as_slice() else {
        panic!("{items:?}");
    };
    assert_eq!(notice.kind, CallNoticeKind::JobCancelled);
    assert_eq!(notice.notice_type, None);
    assert_eq!(notice.child_job_id, Some(id(301)));
    assert_eq!(notice.message.as_deref(), Some("Job was cancelled."));
}

#[tokio::test]
async fn given_a_cancel_notice_without_a_child_job_when_unread_is_read_then_it_is_unreadable() {
    let mut item = notice_item(1, "JOB_CANCELLED");
    item.as_object_mut().unwrap().remove("jobId");
    assert!(matches!(
        read(item).await.unwrap_err(),
        Error::Unreadable(_)
    ));
}

#[tokio::test]
async fn given_a_refusal_notice_when_unread_is_read_then_it_is_a_refusal_with_or_without_a_child_job()
 {
    let mut item = notice_item(1, "REFUSED");
    item["address"] = json!(id(4));
    item["message"] = json!("Job was not created.");
    let items = read(item.clone()).await.unwrap();
    let [UnreadItem::CallNotice(notice)] = items.as_slice() else {
        panic!("{items:?}");
    };
    assert_eq!(notice.kind, CallNoticeKind::Refused);
    assert_eq!(notice.notice_type, None);
    assert_eq!(notice.address, Some(id(4)));
    assert_eq!(notice.child_job_id, Some(id(301)));
    item.as_object_mut().unwrap().remove("jobId");
    let items = read(item).await.unwrap();
    let [UnreadItem::CallNotice(notice)] = items.as_slice() else {
        panic!("{items:?}");
    };
    assert_eq!(notice.kind, CallNoticeKind::Refused);
    assert_eq!(notice.child_job_id, None);
}
