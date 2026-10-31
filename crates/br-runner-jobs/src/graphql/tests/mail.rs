use contract_jobs::graphql_roots::*;
use serde_json::json;

use super::{TEXT, id, ids, jobs, new_attachment, new_mail, success};
use crate::error::{Error, Operation};
use crate::graphql;

#[tokio::test]
async fn given_restart_refusals_when_the_mail_is_sent_then_the_send_is_ok() {
    let scripted = jobs(success(
        SEND_MAIL_ROOT,
        json!({ "restartRefused": [
            { "address": id(13), "reason": "restart_refused", "message": "kept unread" }
        ] }),
    ));
    graphql::send_mail(&*scripted, &ids(), &new_mail(Vec::new()))
        .await
        .unwrap();
}

#[tokio::test]
async fn given_bad_addresses_when_the_mail_is_sent_then_the_refusal_keeps_the_words_of_jobs() {
    let answer = json!({ SEND_MAIL_ROOT: {
        "success": false,
        "reason": "bad_addresses",
        "message": TEXT,
        "badAddresses": [
            { "address": id(13), "reason": "address_unknown", "message": "Nobody has this address." },
            { "address": id(14), "reason": "address_closed", "message": "The agent is gone." },
        ],
        "restartRefused": [],
    } });
    let scripted = jobs(answer);
    let error = graphql::send_mail(&*scripted, &ids(), &new_mail(Vec::new()))
        .await
        .unwrap_err();
    let Error::Refused(refusal) = error else {
        panic!("{error:?}");
    };
    assert_eq!(refusal.operation, Operation::Mail);
    assert_eq!(refusal.reason, "bad_addresses");
    assert!(refusal.message.starts_with(TEXT));
    assert!(refusal.message.contains("Nobody has this address."));
    assert!(refusal.message.contains("The agent is gone."));
    assert!(refusal.message.contains(&id(13).to_string()));
}

#[tokio::test]
async fn given_a_refused_attachment_when_the_mail_is_sent_then_the_mail_is_not_sent() {
    let scripted = jobs(super::refusal(UPLOAD_ATTACHMENT_ROOT, "mail_id_reused"));
    let mail = new_mail(vec![
        new_attachment(1, "one.txt"),
        new_attachment(2, "two.txt"),
    ]);
    let error = graphql::send_mail(&*scripted, &ids(), &mail)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        Error::refused(Operation::Mail, "mail_id_reused", TEXT)
    );
    assert_eq!(super::sent(&scripted).len(), 1);
}

#[tokio::test]
async fn given_a_lost_upload_when_the_mail_is_sent_then_the_error_passes_and_nothing_else_is_sent()
{
    let scripted = crate::test_double::ScriptedJobs::new("runner", "instance-a");
    scripted.push_graphql(Err(Error::lost("cut")));
    let mail = new_mail(vec![new_attachment(1, "one.txt")]);
    let error = graphql::send_mail(&*scripted, &ids(), &mail)
        .await
        .unwrap_err();
    assert_eq!(error, Error::lost("cut"));
    assert_eq!(super::sent(&scripted).len(), 1);
}

#[tokio::test]
async fn given_an_author_and_a_source_conversation_when_the_mail_is_sent_then_they_go_unchanged() {
    let scripted = jobs(success(SEND_MAIL_ROOT, json!({})));
    let mut mail = new_mail(Vec::new());
    mail.author = "  odd author \u{e9}  ".to_owned();
    graphql::send_mail(&*scripted, &ids(), &mail).await.unwrap();
    let input = &super::only_call(&scripted).1["input"];
    assert_eq!(input["author"], json!("  odd author \u{e9}  "));
    assert_eq!(input["sourceConversationId"], json!(id(12)));
}

#[tokio::test]
async fn given_a_mail_when_it_is_read_then_recipients_author_and_attachments_are_returned() {
    let scripted = jobs(success(
        MAIL_ROOT,
        json!({ "mail": {
            "mailId": id(3),
            "from": id(4),
            "author": "Ann",
            "sourceConversationId": id(5),
            "subject": "s",
            "body": "b",
            "to": [id(6)],
            "cc": [id(7), id(8)],
            "attachments": [
                { "attachmentId": id(9), "name": "f.bin", "contentType": "application/x", "size": 12 }
            ],
        } }),
    ));
    let mail = graphql::read_mail(&*scripted, &ids(), id(3), id(6))
        .await
        .unwrap();
    assert_eq!(mail.mail_id, id(3));
    assert_eq!(mail.from, id(4));
    assert_eq!(mail.author, "Ann");
    assert_eq!(mail.source_conversation, Some(id(5)));
    assert_eq!((mail.subject.as_str(), mail.body.as_str()), ("s", "b"));
    assert_eq!(mail.to, vec![id(6)]);
    assert_eq!(mail.cc, vec![id(7), id(8)]);
    assert_eq!(mail.attachments.len(), 1);
    assert_eq!(mail.attachments[0].attachment_id, id(9));
    assert_eq!(
        mail.attachments[0].content_type.as_deref(),
        Some("application/x")
    );
    assert_eq!(mail.attachments[0].size, Some(12));
}
