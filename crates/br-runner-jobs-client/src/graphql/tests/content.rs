use contract_jobs::graphql_roots::*;
use serde_json::json;

use super::{content_value, id, ids, jobs, only_call, success};
use crate::graphql;
use crate::model::{Content, ContentAttachment, ResultRef};

fn expected() -> Content {
    Content {
        message: "hello".to_owned(),
        attachments: vec![ContentAttachment {
            name: "a.txt".to_owned(),
            content_type: "text/plain".to_owned(),
            content: "QQ==".to_owned(),
        }],
    }
}

#[tokio::test]
async fn given_a_kept_payload_when_it_is_read_then_the_content_is_returned_whole() {
    let scripted = jobs(success(PAYLOAD_ROOT, json!({ "content": content_value() })));
    assert_eq!(
        graphql::payload(&*scripted, &ids()).await.unwrap(),
        expected()
    );
}

#[tokio::test]
async fn given_a_result_when_it_is_read_with_keep_then_the_content_is_returned_and_keep_is_sent() {
    let scripted = jobs(success(
        READ_RESULT_ROOT,
        json!({ "content": content_value() }),
    ));
    let content = graphql::read_result(&*scripted, &ids(), ResultRef(id(5)), true)
        .await
        .unwrap();
    assert_eq!(content, expected());
    assert_eq!(only_call(&scripted).1["input"]["keep"], json!(true));
}

#[tokio::test]
async fn given_an_accepted_upload_when_a_payload_or_an_answer_is_sent_then_it_is_ok() {
    let scripted = jobs(success(UPLOAD_PAYLOAD_ROOT, json!({})));
    scripted.push_graphql(Ok(success(UPLOAD_RESULT_ROOT, json!({}))));
    graphql::upload_payload(&*scripted, &ids(), id(9), &expected())
        .await
        .unwrap();
    graphql::upload_result(&*scripted, &ids(), &expected())
        .await
        .unwrap();
}

#[tokio::test]
async fn given_an_attachment_when_it_is_fetched_then_name_type_and_content_are_returned() {
    let scripted = jobs(success(
        ATTACHMENT_ROOT,
        json!({ "attachment": { "name": "a.txt", "contentType": "text/plain", "content": "QQ==" } }),
    ));
    let fetched = graphql::attachment(&*scripted, &ids(), id(3), id(4))
        .await
        .unwrap();
    assert_eq!(fetched, expected().attachments[0]);
}

#[tokio::test]
async fn given_a_content_without_attachments_when_it_is_read_then_the_list_is_empty() {
    let scripted = jobs(success(
        PAYLOAD_ROOT,
        json!({ "content": { "message": "x", "attachments": [] } }),
    ));
    let content = graphql::payload(&*scripted, &ids()).await.unwrap();
    assert_eq!(content.attachments, Vec::new());
}
