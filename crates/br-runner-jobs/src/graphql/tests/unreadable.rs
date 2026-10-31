use contract_jobs::graphql_roots::*;
use serde_json::{Value, json};

use super::unread::{mail_item, notice_item, reply};
use super::{id, ids, jobs, success};
use crate::backend::Backend;
use crate::error::Error;
use crate::graphql::{self, Read};
use crate::model::{CallNoticeKind, Content, NewMailInput, ResultRef, UnreadItem};

pub(super) fn assert_unreadable(error: &Error) {
    let Error::Unreadable(text) = error else {
        panic!("{error:?}");
    };
    for secret in [id(1), id(2)] {
        assert!(!text.contains(&secret.to_string()), "{text}");
    }
    assert!(!text.contains("instance-a"), "{text}");
}

async fn unread_with(item: Value) -> Error {
    let scripted = jobs(reply(vec![item]));
    graphql::unread(&*scripted, &ids(), None).await.unwrap_err()
}

macro_rules! unreadable_reply {
    ($name:ident, $answer:expr, |$b:ident, $w:ident| $call:expr) => {
        #[tokio::test]
        async fn $name() {
            let scripted = jobs($answer);
            let ($b, $w) = (&*scripted as &dyn Backend, ids());
            let error = $call.await.unwrap_err();
            assert_unreadable(&error);
        }
    };
}

fn content() -> Content {
    Content {
        message: "m".to_owned(),
        attachments: Vec::new(),
    }
}

fn mail() -> NewMailInput {
    super::new_mail(Vec::new())
}

unreadable_reply!(
    given_no_root_when_the_payload_is_read_then_it_is_unreadable,
    json!({}),
    |b, w| graphql::payload(b, &w)
);
unreadable_reply!(
    given_no_root_when_a_payload_is_uploaded_then_it_is_unreadable,
    json!({}),
    |b, w| graphql::upload_payload(b, &w, id(9), &content())
);
unreadable_reply!(
    given_no_root_when_an_answer_is_uploaded_then_it_is_unreadable,
    json!({}),
    |b, w| graphql::upload_result(b, &w, &content())
);
unreadable_reply!(
    given_no_root_when_a_result_is_read_then_it_is_unreadable,
    json!({}),
    |b, w| graphql::read_result(b, &w, ResultRef(id(5)), false)
);
unreadable_reply!(
    given_no_root_when_a_state_is_saved_then_it_is_unreadable,
    json!({}),
    |b, w| graphql::save_state(b, &w, &Value::Null)
);
unreadable_reply!(
    given_no_root_when_the_state_is_read_then_it_is_unreadable,
    json!({}),
    |b, w| graphql::saved_state(b, &w)
);
unreadable_reply!(
    given_no_root_when_a_mail_is_sent_then_it_is_unreadable,
    json!({}),
    |b, w| graphql::send_mail(b, &w, &mail())
);
unreadable_reply!(
    given_no_root_when_unread_is_read_then_it_is_unreadable,
    json!({}),
    |b, w| graphql::unread(b, &w, None)
);
unreadable_reply!(
    given_no_root_when_a_mail_is_read_then_it_is_unreadable,
    json!({}),
    |b, w| graphql::read_mail(b, &w, id(3), id(4))
);
unreadable_reply!(
    given_no_root_when_an_attachment_is_fetched_then_it_is_unreadable,
    json!({}),
    |b, w| graphql::attachment(b, &w, id(3), id(4))
);
unreadable_reply!(
    given_a_null_data_when_the_payload_is_read_then_it_is_unreadable,
    Value::Null,
    |b, w| graphql::payload(b, &w)
);
unreadable_reply!(
    given_a_root_that_is_not_an_object_then_it_is_unreadable,
    json!({ PAYLOAD_ROOT: "yes" }),
    |b, w| graphql::payload(b, &w)
);
unreadable_reply!(
    given_no_success_flag_then_it_is_unreadable,
    json!({ SAVE_STATE_ROOT: {} }),
    |b, w| graphql::save_state(b, &w, &Value::Null)
);
unreadable_reply!(
    given_a_success_flag_of_the_wrong_type_then_it_is_unreadable,
    json!({ SAVE_STATE_ROOT: { "success": "true" } }),
    |b, w| graphql::save_state(b, &w, &Value::Null)
);
unreadable_reply!(
    given_a_refusal_without_a_reason_then_it_is_unreadable,
    json!({ SAVE_STATE_ROOT: { "success": false, "message": "m" } }),
    |b, w| graphql::save_state(b, &w, &Value::Null)
);
unreadable_reply!(
    given_a_refusal_without_a_text_then_it_is_unreadable,
    json!({ SAVE_STATE_ROOT: { "success": false, "reason": "r", "message": null } }),
    |b, w| graphql::save_state(b, &w, &Value::Null)
);
unreadable_reply!(
    given_a_success_without_content_when_the_payload_is_read_then_it_is_unreadable,
    success(PAYLOAD_ROOT, json!({ "content": null })),
    |b, w| graphql::payload(b, &w)
);
unreadable_reply!(
    given_a_content_without_message_then_it_is_unreadable,
    success(PAYLOAD_ROOT, json!({ "content": { "attachments": [] } })),
    |b, w| graphql::payload(b, &w)
);
unreadable_reply!(
    given_an_attachment_of_the_wrong_type_then_it_is_unreadable,
    success(
        PAYLOAD_ROOT,
        json!({ "content": { "message": "m", "attachments": [{ "name": 3 }] } })
    ),
    |b, w| graphql::payload(b, &w)
);
unreadable_reply!(
    given_a_success_without_attachment_when_it_is_fetched_then_it_is_unreadable,
    success(ATTACHMENT_ROOT, json!({})),
    |b, w| graphql::attachment(b, &w, id(3), id(4))
);
unreadable_reply!(
    given_a_state_read_without_the_state_then_it_is_unreadable,
    success(
        SAVED_STATE_ROOT,
        json!({ "agentIds": [], "mailAddresses": [] })
    ),
    |b, w| graphql::saved_state(b, &w)
);
unreadable_reply!(
    given_a_state_read_with_a_bad_agent_id_then_it_is_unreadable,
    success(
        SAVED_STATE_ROOT,
        json!({ "state": {}, "agentIds": [format!("{}-x", id(1))], "mailAddresses": [] })
    ),
    |b, w| graphql::saved_state(b, &w)
);
unreadable_reply!(
    given_a_state_read_without_addresses_then_it_is_unreadable,
    success(SAVED_STATE_ROOT, json!({ "state": {}, "agentIds": [] })),
    |b, w| graphql::saved_state(b, &w)
);
unreadable_reply!(
    given_a_success_without_mail_when_it_is_read_then_it_is_unreadable,
    success(MAIL_ROOT, json!({})),
    |b, w| graphql::read_mail(b, &w, id(3), id(4))
);
unreadable_reply!(
    given_unread_without_items_then_it_is_unreadable,
    success(UNREAD_ROOT, json!({})),
    |b, w| graphql::unread(b, &w, None)
);
unreadable_reply!(
    given_unread_items_that_are_not_a_list_then_it_is_unreadable,
    success(UNREAD_ROOT, json!({ "items": {} })),
    |b, w| graphql::unread(b, &w, None)
);

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

#[tokio::test]
async fn given_a_notice_without_its_child_job_id_then_unread_is_unreadable() {
    let mut item = notice_item(1, "ACCEPTED");
    item["jobId"] = Value::Null;
    assert_unreadable(&unread_with(item).await);
    let mut item = notice_item(1, "ACCEPTED");
    item.as_object_mut().unwrap().remove("jobId");
    assert_unreadable(&unread_with(item).await);
}

#[tokio::test]
async fn given_a_notice_without_its_type_then_unread_is_unreadable() {
    let mut item = notice_item(1, "ACCEPTED");
    item["noticeType"] = Value::Null;
    assert_unreadable(&unread_with(item).await);
}

#[tokio::test]
async fn given_a_mail_without_author_then_unread_is_unreadable_and_never_defaults() {
    for author in [Value::Null, json!(""), json!("   "), json!(7)] {
        let mut item = mail_item(1);
        item["author"] = author;
        assert_unreadable(&unread_with(item).await);
    }
    let mut item = mail_item(1);
    item.as_object_mut().unwrap().remove("author");
    assert_unreadable(&unread_with(item).await);
}

#[tokio::test]
async fn given_a_mail_without_its_sender_or_body_then_unread_is_unreadable() {
    for key in ["from", "body", "subject", "mailId", "attachments"] {
        let mut item = mail_item(1);
        item[key] = Value::Null;
        assert_unreadable(&unread_with(item).await);
    }
}

#[tokio::test]
async fn given_a_cc_notice_without_mail_id_then_unread_is_unreadable() {
    let item = json!({ "itemId": id(101), "itemKind": "CC_NOTICE", "subject": "s" });
    assert_unreadable(&unread_with(item).await);
}

#[tokio::test]
async fn given_a_bad_failure_report_or_answer_then_unread_is_unreadable() {
    for (key, value) in [
        ("failureReport", json!("text")),
        (
            "failureReport",
            json!({ "kind": "ODD", "reason_code": "x" }),
        ),
        ("failureReport", json!({ "kind": "PERMANENT" })),
        ("answer", json!([])),
        ("answer", json!({ "message": "m" })),
        ("position", json!("two")),
        ("resultId", json!("nope")),
    ] {
        let mut item = notice_item(1, "RUN_COMPLETED");
        item[key] = value;
        assert_unreadable(&unread_with(item).await);
    }
}

#[tokio::test]
async fn given_one_bad_item_among_good_ones_then_the_whole_listing_of_an_address_is_unreadable() {
    let mut bad = mail_item(2);
    bad["author"] = Value::Null;
    let scripted = jobs(reply(vec![mail_item(1), bad, mail_item(3)]));
    let error = graphql::unread(&*scripted, &ids(), None).await.unwrap_err();
    assert_unreadable(&error);
}

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

#[tokio::test]
async fn given_ids_of_the_work_inside_a_bad_value_then_the_text_of_the_error_does_not_echo_them() {
    let mut item = mail_item(1);
    item["from"] = json!(format!("{} {}", id(1), id(2)));
    let error = unread_with(item).await;
    assert_unreadable(&error);
    assert!(!error.to_string().contains(&id(2).to_string()));
}
