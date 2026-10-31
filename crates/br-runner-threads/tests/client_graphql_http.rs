mod support;

use br_llm_messages::{Conversation, ConversationId, Turn};
use br_runner_threads::{
    ContextContent, ContextQuery, Error, FailTitleJob, SetTitle, UploadRequest,
};
use serde_json::{Value, json};
use support::nats_fixtures::BEARER;
use support::nats_gateway::gateway_rig;
use support::nats_http::Answer;
use support::{agent_id, input_turn, notice_entry, v7};
use uuid::Uuid;

fn data(value: Value) -> Answer {
    Answer::Json(200, json!({ "data": value }))
}

fn conversation_document() -> Value {
    let id = ConversationId::from_uuid(v7()).expect("conversation id");
    let turn = Turn::InputTurn(input_turn(vec![notice_entry("earlier")]));
    let conversation = Conversation::from_parts(id, vec![turn]).expect("conversation");
    serde_json::from_str(&conversation.to_json().expect("json")).expect("a document")
}

fn context_answer(thread_id: Uuid) -> Answer {
    data(json!({ "threadsContext": {
        "threadId": thread_id,
        "conversation": conversation_document(),
        "projectedThread": null,
        "members": [{ "userId": v7(), "displayName": "Ada", "inRoster": true }],
        "resources": [{ "key": "kms", "ids": ["km-1"] }],
    }}))
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn the_context_of_a_thread_is_read_over_the_gateway_with_the_bearer_of_the_connection() {
    let thread_id = v7();
    let rig = gateway_rig(move |_| context_answer(thread_id)).await;

    let context = rig
        .client
        .read_context(&ContextQuery {
            thread_id,
            agent: None,
        })
        .await
        .expect("read")
        .expect("the thread exists");

    assert_eq!(context.thread_id, thread_id);
    assert!(matches!(context.content, ContextContent::Conversation(_)));
    assert_eq!(context.members.len(), 1);
    assert_eq!(context.members[0].display_name.as_deref(), Some("Ada"));
    assert_eq!(context.resources[0].key, "kms");
    let sent = rig.gateway.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, "POST");
    assert_eq!(sent[0].target, "/graphql");
    let bearer = format!("Bearer {BEARER}");
    assert_eq!(sent[0].header("authorization"), Some(bearer.as_str()));
    let body = sent[0].json();
    assert!(
        body["query"]
            .as_str()
            .expect("a document")
            .contains("threadsContext")
    );
    assert_eq!(body["variables"]["threadId"], json!(thread_id));
    assert_eq!(body["variables"]["agentId"], Value::Null);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_named_agent_travels_as_a_variable_and_a_context_without_its_projection_is_unreadable() {
    let thread_id = v7();
    let agent = agent_id();
    let rig = gateway_rig(move |_| context_answer(thread_id)).await;

    let result = rig
        .client
        .read_context(&ContextQuery {
            thread_id,
            agent: Some(agent),
        })
        .await;

    assert!(matches!(result, Err(Error::Unreadable(_))), "{result:?}");
    let body = rig.gateway.requests()[0].json();
    assert_eq!(
        body["variables"]["agentId"],
        json!(agent.as_uuid().to_string())
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_thread_the_gateway_does_not_know_reads_as_none() {
    let rig = gateway_rig(|_| data(json!({ "threadsContext": null }))).await;

    let context = rig
        .client
        .read_context(&ContextQuery {
            thread_id: v7(),
            agent: None,
        })
        .await;

    assert!(matches!(context, Ok(None)), "{context:?}");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_response_without_the_context_field_is_unreadable() {
    let rig = gateway_rig(|_| data(json!({ "somethingElse": 1 }))).await;

    let result = rig
        .client
        .read_context(&ContextQuery {
            thread_id: v7(),
            agent: None,
        })
        .await;

    assert!(matches!(result, Err(Error::Unreadable(_))), "{result:?}");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_title_is_set_through_a_mutation_that_must_be_acknowledged() {
    let rig = gateway_rig(|_| data(json!({ "threadsSetTitle": { "success": true } }))).await;
    let title = SetTitle {
        thread_id: v7(),
        title_job_id: v7(),
        text: "A title".to_owned(),
    };

    rig.client.set_title(&title).await.expect("acknowledged");

    let body = rig.gateway.requests()[0].json();
    assert!(
        body["query"]
            .as_str()
            .expect("a document")
            .contains("threadsSetTitle")
    );
    assert_eq!(body["variables"]["threadId"], json!(title.thread_id));
    assert_eq!(body["variables"]["jobId"], json!(title.title_job_id));
    assert_eq!(body["variables"]["title"], "A title");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_title_mutation_that_is_not_acknowledged_is_unreadable() {
    let rig = gateway_rig(|_| data(json!({ "threadsSetTitle": { "success": false } }))).await;
    let title = SetTitle {
        thread_id: v7(),
        title_job_id: v7(),
        text: "A title".to_owned(),
    };

    let result = rig.client.set_title(&title).await;

    assert!(matches!(result, Err(Error::Unreadable(_))), "{result:?}");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_failed_title_job_is_reported_with_its_code_and_message() {
    let rig = gateway_rig(|_| data(json!({ "threadsFailTitleJob": { "success": true } }))).await;
    let fail = FailTitleJob {
        thread_id: v7(),
        title_job_id: v7(),
        code: "MODEL_DOWN".to_owned(),
        message: Some("the model is down".to_owned()),
    };

    rig.client
        .fail_title_job(&fail)
        .await
        .expect("acknowledged");

    let variables = rig.gateway.requests()[0].json()["variables"].clone();
    assert_eq!(variables["reasonCode"], "MODEL_DOWN");
    assert_eq!(variables["message"], "the model is down");
    assert_eq!(variables["jobId"], json!(fail.title_job_id));
}

fn upload_request() -> UploadRequest {
    UploadRequest {
        artifact_id: v7(),
        thread_id: v7(),
        name: "report.pdf".to_owned(),
        media_type: "application/pdf".to_owned(),
        size: 1024,
        sha256: "ab".repeat(32),
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn an_upload_access_is_requested_and_read_back() {
    let access = json!({ "url": "https://store.example/up", "fields": { "key": "k-1" } });
    let rig =
        gateway_rig(move |_| data(json!({ "threadsRequestArtifactUpload": access.clone() }))).await;
    let request = upload_request();

    let granted = rig.client.request_upload(&request).await.expect("granted");

    assert_eq!(granted.url, "https://store.example/up");
    assert_eq!(granted.fields["key"], "k-1");
    let variables = rig.gateway.requests()[0].json()["variables"].clone();
    assert_eq!(variables["artifactId"], json!(request.artifact_id));
    assert_eq!(variables["size"], 1024);
    assert_eq!(variables["mediaType"], "application/pdf");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn an_upload_for_an_artifact_id_that_is_not_a_uuid_v7_sends_nothing() {
    let rig = gateway_rig(|_| data(json!({}))).await;
    let mut request = upload_request();
    request.artifact_id = Uuid::from_u128(1);

    let result = rig.client.request_upload(&request).await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert!(rig.gateway.requests().is_empty());
}
