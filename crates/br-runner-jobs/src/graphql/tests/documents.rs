use contract_jobs::graphql_roots::*;
use serde_json::{Value, json};

use super::{
    id, ids, jobs, new_attachment, new_mail, only_call, refusal, sent, success, work_json,
};
use crate::graphql;
use crate::model::{Content, ResultRef};

fn assert_root(document: &str, root: &str) {
    assert!(
        document.contains(&format!("{root}(")),
        "{document} must call {root}"
    );
}

fn reply(root: &str) -> Value {
    refusal(root, "run_ended")
}

#[tokio::test]
async fn given_a_work_when_the_payload_is_read_then_the_query_carries_the_work_ids() {
    let scripted = jobs(reply(PAYLOAD_ROOT));
    let _ = graphql::payload(&*scripted, &ids()).await;
    let (document, variables) = only_call(&scripted);
    assert_root(&document, PAYLOAD_ROOT);
    assert!(document.starts_with("query"));
    assert_eq!(variables, json!({ "work": work_json() }));
}

#[tokio::test]
async fn given_a_payload_when_it_is_uploaded_then_the_mutation_carries_the_job_the_content_and_the_work()
 {
    let scripted = jobs(reply(UPLOAD_PAYLOAD_ROOT));
    let content = Content {
        message: "m".to_owned(),
        attachments: vec![crate::model::ContentAttachment {
            name: "n".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "QQ==".to_owned(),
        }],
    };
    let _ = graphql::upload_payload(&*scripted, &ids(), id(9), &content).await;
    let (document, variables) = only_call(&scripted);
    assert_root(&document, UPLOAD_PAYLOAD_ROOT);
    assert!(document.starts_with("mutation"));
    assert_eq!(
        variables,
        json!({ "input": {
            "forJobId": id(9),
            "content": { "message": "m", "attachments": [
                { "name": "n", "contentType": "text/plain", "content": "QQ==" }
            ] },
            "work": work_json(),
        } })
    );
}

#[tokio::test]
async fn given_an_answer_when_it_is_uploaded_then_the_whole_answer_goes_with_the_work() {
    let scripted = jobs(reply(UPLOAD_RESULT_ROOT));
    let answer = Content {
        message: "done".to_owned(),
        attachments: Vec::new(),
    };
    let _ = graphql::upload_result(&*scripted, &ids(), &answer).await;
    let (document, variables) = only_call(&scripted);
    assert_root(&document, UPLOAD_RESULT_ROOT);
    assert_eq!(
        variables,
        json!({ "input": { "work": work_json(), "answer": { "message": "done", "attachments": [] } } })
    );
}

#[tokio::test]
async fn given_a_keep_flag_when_a_result_is_read_then_the_flag_is_sent_as_given() {
    for keep in [true, false] {
        let scripted = jobs(reply(READ_RESULT_ROOT));
        let _ = graphql::read_result(&*scripted, &ids(), ResultRef(id(5)), keep).await;
        let (document, variables) = only_call(&scripted);
        assert_root(&document, READ_RESULT_ROOT);
        assert_eq!(
            variables,
            json!({ "input": { "work": work_json(), "resultId": id(5), "keep": keep } })
        );
    }
}

#[tokio::test]
async fn given_a_mail_and_an_attachment_when_it_is_fetched_then_the_ids_are_sent() {
    let scripted = jobs(reply(ATTACHMENT_ROOT));
    let _ = graphql::attachment(&*scripted, &ids(), id(3), id(4)).await;
    let (document, variables) = only_call(&scripted);
    assert_root(&document, ATTACHMENT_ROOT);
    assert_eq!(
        variables,
        json!({ "work": work_json(), "mailId": id(3), "attachmentId": id(4) })
    );
}

#[tokio::test]
async fn given_a_state_when_it_is_saved_then_the_state_goes_as_json_with_the_work() {
    let scripted = jobs(reply(SAVE_STATE_ROOT));
    let state = json!({ "agents": [1, 2], "nested": { "a": null } });
    let _ = graphql::save_state(&*scripted, &ids(), &state).await;
    let (document, variables) = only_call(&scripted);
    assert_root(&document, SAVE_STATE_ROOT);
    assert_eq!(
        variables,
        json!({ "input": { "work": work_json(), "state": state } })
    );
}

#[tokio::test]
async fn given_a_work_when_the_saved_state_is_read_then_only_the_work_is_sent() {
    let scripted = jobs(reply(SAVED_STATE_ROOT));
    let _ = graphql::saved_state(&*scripted, &ids()).await;
    let (document, variables) = only_call(&scripted);
    assert_root(&document, SAVED_STATE_ROOT);
    assert_eq!(variables, json!({ "work": work_json() }));
}

#[tokio::test]
async fn given_no_address_when_unread_is_read_then_the_address_is_null_and_with_one_it_is_sent() {
    let scripted = jobs(reply(UNREAD_ROOT));
    let _ = graphql::unread(&*scripted, &ids(), None).await;
    let (document, variables) = only_call(&scripted);
    assert_root(&document, UNREAD_ROOT);
    assert_eq!(variables, json!({ "work": work_json(), "address": null }));
    let scripted = jobs(reply(UNREAD_ROOT));
    let _ = graphql::unread(&*scripted, &ids(), Some(id(4))).await;
    assert_eq!(
        only_call(&scripted).1,
        json!({ "work": work_json(), "address": id(4) })
    );
}

#[tokio::test]
async fn given_a_mail_id_and_an_address_when_the_mail_is_read_then_both_are_sent() {
    let scripted = jobs(reply(MAIL_ROOT));
    let _ = graphql::read_mail(&*scripted, &ids(), id(3), id(4)).await;
    let (document, variables) = only_call(&scripted);
    assert_root(&document, MAIL_ROOT);
    assert_eq!(
        variables,
        json!({ "work": work_json(), "mailId": id(3), "address": id(4) })
    );
}

#[tokio::test]
async fn given_a_mail_with_attachments_when_it_is_sent_then_each_attachment_is_uploaded_first_in_order()
 {
    let scripted = jobs(success(UPLOAD_ATTACHMENT_ROOT, json!({})));
    scripted.push_graphql(Ok(success(UPLOAD_ATTACHMENT_ROOT, json!({}))));
    scripted.push_graphql(Ok(success(SEND_MAIL_ROOT, json!({ "restartRefused": [] }))));
    let mail = new_mail(vec![
        new_attachment(1, "one.txt"),
        new_attachment(2, "two.txt"),
    ]);
    graphql::send_mail(&*scripted, &ids(), &mail).await.unwrap();
    let calls = sent(&scripted);
    assert_eq!(calls.len(), 3);
    for (index, (document, variables)) in calls.iter().take(2).enumerate() {
        let n = index as u128 + 1;
        assert_root(document, UPLOAD_ATTACHMENT_ROOT);
        assert_eq!(
            *variables,
            json!({ "input": {
                "work": work_json(),
                "mailId": id(10),
                "attachmentId": id(20 + n),
                "fromAddress": id(11),
                "name": if n == 1 { "one.txt" } else { "two.txt" },
                "contentType": "text/plain",
                "content": format!("content-{n}"),
            } })
        );
    }
    assert_root(&calls[2].0, SEND_MAIL_ROOT);
    assert_eq!(
        calls[2].1,
        json!({ "input": {
            "work": work_json(),
            "mailId": id(10),
            "fromAddress": id(11),
            "author": "An Author",
            "sourceConversationId": id(12),
            "subject": "subject",
            "body": "body",
            "to": [id(13), id(14)],
            "cc": [id(15)],
            "attachments": [
                { "attachmentId": id(21), "name": "one.txt" },
                { "attachmentId": id(22), "name": "two.txt" },
            ],
        } })
    );
}

#[tokio::test]
async fn given_any_call_then_neither_the_documents_nor_the_variables_name_a_passport() {
    let scripted = jobs(reply(SEND_MAIL_ROOT));
    let _ = graphql::send_mail(&*scripted, &ids(), &new_mail(Vec::new())).await;
    let (document, variables) = only_call(&scripted);
    assert!(!document.to_lowercase().contains("passport"));
    assert!(!variables.to_string().to_lowercase().contains("passport"));
}
