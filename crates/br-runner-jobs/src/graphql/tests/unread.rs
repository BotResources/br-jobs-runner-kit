use contract_jobs::graphql_roots::UNREAD_ROOT;
use serde_json::{Value, json};

use super::{id, ids, jobs, success};
use crate::graphql;
use crate::model::{CallNoticeKind, Content, FailureKind, ResultRef, UnreadItem};

pub(super) fn reply(items: Vec<Value>) -> Value {
    success(UNREAD_ROOT, json!({ "items": items }))
}

pub(super) fn mail_item(n: u128) -> Value {
    json!({
        "itemId": id(100 + n), "seq": n, "itemKind": "MAIL", "address": id(4), "runId": id(2),
        "mailId": id(200 + n), "subject": "subject", "from": id(5), "author": "Ann",
        "sourceConversationId": id(6), "body": "body",
        "attachments": [{ "attachmentId": id(7), "name": "f", "contentType": "text/plain", "size": 3 }],
    })
}

pub(super) fn notice_item(n: u128, notice_type: &str) -> Value {
    json!({
        "itemId": id(100 + n), "seq": n, "itemKind": "NOTICE", "address": null, "runId": id(2),
        "noticeType": notice_type, "jobId": id(300 + n),
    })
}

async fn read(items: Vec<Value>) -> Vec<UnreadItem> {
    let scripted = jobs(reply(items));
    graphql::unread(&*scripted, &ids(), None).await.unwrap()
}

#[tokio::test]
async fn given_a_direct_mail_when_unread_is_read_then_the_mail_has_its_content_author_and_source() {
    let items = read(vec![mail_item(1)]).await;
    let UnreadItem::DirectMail {
        item_id,
        address,
        mail,
    } = &items[0]
    else {
        panic!("{items:?}");
    };
    assert_eq!(*item_id, id(101));
    assert_eq!(*address, Some(id(4)));
    assert_eq!(mail.mail_id, id(201));
    assert_eq!(mail.from, id(5));
    assert_eq!(mail.author, "Ann");
    assert_eq!(mail.source_conversation, Some(id(6)));
    assert_eq!(
        (mail.subject.as_str(), mail.body.as_str()),
        ("subject", "body")
    );
    assert_eq!(mail.attachments[0].attachment_id, id(7));
    assert_eq!(mail.attachments[0].size, Some(3));
}

#[tokio::test]
async fn given_a_direct_mail_when_unread_is_read_then_the_recipient_lists_are_empty_because_the_item_has_none()
 {
    let items = read(vec![mail_item(1)]).await;
    let UnreadItem::DirectMail { mail, .. } = &items[0] else {
        panic!("{items:?}");
    };
    assert!(mail.to.is_empty() && mail.cc.is_empty());
}

#[tokio::test]
async fn given_a_cc_notice_when_unread_is_read_then_the_mail_id_and_subject_are_given() {
    let item = json!({
        "itemId": id(101), "seq": 1, "itemKind": "CC_NOTICE", "address": id(4),
        "mailId": id(201), "subject": "cc subject", "attachments": null, "noticeType": null,
    });
    let items = read(vec![item]).await;
    assert_eq!(
        items,
        vec![UnreadItem::CcNotice {
            item_id: id(101),
            address: Some(id(4)),
            mail_id: id(201),
            subject: "cc subject".to_owned(),
        }]
    );
}

#[tokio::test]
async fn given_each_notice_type_when_unread_is_read_then_the_kind_and_the_child_job_are_mapped() {
    let table = [
        ("ACCEPTED", CallNoticeKind::Accepted),
        ("QUEUE_POSITION", CallNoticeKind::QueuePosition),
        ("STARTED", CallNoticeKind::RunStarted),
        ("RUN_COMPLETED", CallNoticeKind::RunCompleted),
        ("JOB_FAILED", CallNoticeKind::JobFailed),
    ];
    for (n, (wire, kind)) in table.into_iter().enumerate() {
        let n = n as u128 + 1;
        let items = read(vec![notice_item(n, wire)]).await;
        let UnreadItem::CallNotice(notice) = &items[0] else {
            panic!("{items:?}");
        };
        assert_eq!(notice.kind, kind);
        assert_eq!(notice.child_job_id, Some(id(300 + n)));
        assert_eq!(notice.item_id, id(100 + n));
        assert_eq!(notice.address, None);
        assert_eq!(notice.position, None);
        assert_eq!(notice.message, None);
        assert!(notice.report.is_none() && notice.inline.is_none() && notice.result_ref.is_none());
    }
}

#[tokio::test]
async fn given_a_queue_position_notice_when_unread_is_read_then_the_position_and_message_are_given()
{
    let mut item = notice_item(1, "QUEUE_POSITION");
    item["position"] = json!(4);
    item["message"] = json!("You are fourth.");
    let items = read(vec![item]).await;
    let UnreadItem::CallNotice(notice) = &items[0] else {
        panic!("{items:?}");
    };
    assert_eq!(notice.position, Some(4));
    assert_eq!(notice.message.as_deref(), Some("You are fourth."));
}

#[tokio::test]
async fn given_a_failed_notice_when_unread_is_read_then_the_report_is_decoded() {
    let mut item = notice_item(1, "JOB_FAILED");
    item["failureReport"] = json!({
        "kind": "TRANSIENT", "reason_code": "llm_down", "params": { "a": 1 }, "diagnostic": "d",
    });
    item["message"] = json!("It failed.");
    let items = read(vec![item]).await;
    let UnreadItem::CallNotice(notice) = &items[0] else {
        panic!("{items:?}");
    };
    let report = notice.report.as_ref().unwrap();
    assert_eq!(report.kind, Some(FailureKind::Transient));
    assert_eq!(report.reason_code, "llm_down");
    assert_eq!(report.params, json!({ "a": 1 }));
    assert_eq!(report.diagnostic, json!("d"));
    assert_eq!(notice.message.as_deref(), Some("It failed."));
}

#[tokio::test]
async fn given_a_report_without_kind_when_unread_is_read_then_the_kind_stays_unknown() {
    let mut item = notice_item(1, "JOB_FAILED");
    item["failureReport"] = json!({ "reason_code": "x" });
    let items = read(vec![item]).await;
    let UnreadItem::CallNotice(notice) = &items[0] else {
        panic!("{items:?}");
    };
    assert_eq!(notice.report.as_ref().unwrap().kind, None);
}

#[tokio::test]
async fn given_an_inline_answer_when_unread_is_read_then_the_content_is_decoded_with_snake_case_attachments()
 {
    let mut item = notice_item(1, "RUN_COMPLETED");
    item["answer"] = json!({
        "message": "ok",
        "attachments": [{ "name": "n", "content_type": "text/plain", "content": "QQ==" }],
    });
    let items = read(vec![item]).await;
    let UnreadItem::CallNotice(notice) = &items[0] else {
        panic!("{items:?}");
    };
    let inline: &Content = notice.inline.as_ref().unwrap();
    assert_eq!(inline.message, "ok");
    assert_eq!(inline.attachments[0].content_type, "text/plain");
}

#[tokio::test]
async fn given_a_result_id_when_unread_is_read_then_the_result_reference_is_given() {
    let mut item = notice_item(1, "RUN_COMPLETED");
    item["resultId"] = json!(id(55));
    let items = read(vec![item]).await;
    let UnreadItem::CallNotice(notice) = &items[0] else {
        panic!("{items:?}");
    };
    assert_eq!(notice.result_ref, Some(ResultRef(id(55))));
    assert!(notice.inline.is_none());
}

#[tokio::test]
async fn given_items_in_any_order_when_unread_is_read_then_the_order_of_jobs_is_kept() {
    let items = read(vec![
        notice_item(3, "STARTED"),
        mail_item(1),
        notice_item(2, "ACCEPTED"),
    ])
    .await;
    let order: Vec<_> = items.iter().map(UnreadItem::item_id).collect();
    assert_eq!(order, vec![id(103), id(101), id(102)]);
}

#[tokio::test]
async fn given_no_item_when_unread_is_read_then_the_list_is_empty() {
    assert_eq!(read(Vec::new()).await, Vec::new());
}

#[tokio::test]
async fn given_an_item_when_it_is_read_then_its_address_is_available_on_the_item() {
    let items = read(vec![mail_item(1), notice_item(2, "ACCEPTED")]).await;
    assert_eq!(items[0].address(), Some(id(4)));
    assert_eq!(items[1].address(), None);
}
