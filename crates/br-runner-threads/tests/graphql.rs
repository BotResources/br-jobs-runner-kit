mod support;

use br_runner_threads::transport::HttpReply;
use br_runner_threads::{Error, FailTitleJob, SetTitle, UploadRequest};
use serde_json::{Value, json};
use support::*;

async fn failure_of(status: u16, body: Value) -> Error {
    let world = world();
    world.graphql.reply_with(reply(status, body));
    world
        .client
        .read_context(&query(&world, false))
        .await
        .expect_err("an error")
}

fn coded(code: &str) -> Value {
    json!({ "message": "x", "extensions": { "code": code, "limit": 3 } })
}

#[tokio::test]
async fn graphql_errors_map_to_refusals_with_their_code_and_params() {
    match failure_of(
        200,
        json!({ "errors": [coded("THREAD_NOT_FOUND")], "data": null }),
    )
    .await
    {
        Error::Refused(refusal) => {
            assert_eq!(refusal.code, "THREAD_NOT_FOUND");
            assert_eq!(refusal.params["limit"], 3);
            assert_eq!(refusal.command_id, None);
        }
        other => panic!("refused, not {other:?}"),
    }
    let first = json!({ "errors": [{ "message": "no code" }, coded("A"), coded("B")] });
    match failure_of(200, first).await {
        Error::Refused(refusal) => assert_eq!(refusal.code, "A"),
        other => panic!("refused, not {other:?}"),
    }
}

#[tokio::test]
async fn unauthenticated_wins_over_any_other_code_and_a_401_is_unauthenticated() {
    let both = json!({ "errors": [coded("FORBIDDEN"), coded("UNAUTHENTICATED")] });
    assert_eq!(failure_of(200, both.clone()).await, Error::Unauthenticated);
    assert_eq!(failure_of(500, both).await, Error::Unauthenticated);
    assert_eq!(failure_of(401, json!({})).await, Error::Unauthenticated);
    let world = world();
    world.graphql.reply_with(Ok(HttpReply {
        status: 401,
        body: b"<html>".to_vec(),
    }));
    assert_eq!(
        world
            .client
            .read_context(&query(&world, false))
            .await
            .expect_err("error"),
        Error::Unauthenticated
    );
}

#[tokio::test]
async fn a_403_without_a_code_is_a_forbidden_refusal_and_other_failures_lose_the_answer() {
    match failure_of(403, json!({ "error": "denied" })).await {
        Error::Refused(refusal) => assert_eq!(refusal.code, "FORBIDDEN"),
        other => panic!("refused, not {other:?}"),
    }
    assert!(matches!(
        failure_of(502, json!({ "error": "bad gateway" })).await,
        Error::AnswerLost(_)
    ));
    let world = world();
    world.graphql.reply_with(Ok(HttpReply {
        status: 500,
        body: b"oops".to_vec(),
    }));
    assert!(matches!(
        world.client.read_context(&query(&world, false)).await,
        Err(Error::AnswerLost(_))
    ));
    match failure_of(503, json!({ "errors": [coded("BUSY")] })).await {
        Error::Refused(refusal) => assert_eq!(refusal.code, "BUSY"),
        other => panic!("a coded error on a non-2xx is a refusal, not {other:?}"),
    }
}

#[tokio::test]
async fn a_2xx_error_without_a_code_or_without_data_is_unreadable() {
    assert!(matches!(
        failure_of(200, json!({ "errors": [{ "message": "boom" }] })).await,
        Error::Unreadable(_)
    ));
    assert!(matches!(
        failure_of(200, json!({})).await,
        Error::Unreadable(_)
    ));
    let world = world();
    world.graphql.reply_with(Ok(HttpReply {
        status: 200,
        body: b"not json".to_vec(),
    }));
    assert!(matches!(
        world.client.read_context(&query(&world, false)).await,
        Err(Error::Unreadable(_))
    ));
}

#[tokio::test]
async fn the_transport_errors_pass_through_unchanged() {
    let world = world();
    world
        .graphql
        .reply_with(Err(Error::Unavailable("refused".to_owned())));
    world
        .graphql
        .reply_with(Err(Error::Auth("no token".to_owned())));
    assert_eq!(
        world
            .client
            .read_context(&query(&world, false))
            .await
            .expect_err("error"),
        Error::Unavailable("refused".to_owned())
    );
    assert_eq!(
        world
            .client
            .read_context(&query(&world, false))
            .await
            .expect_err("error"),
        Error::Auth("no token".to_owned())
    );
}

#[tokio::test]
async fn the_title_is_set_and_the_title_job_is_failed_with_the_variables_sent() {
    let world = world();
    let job = v7();
    world.graphql.reply_with(reply(
        200,
        json!({ "data": { "threadsSetTitle": { "success": true } } }),
    ));
    world
        .client
        .set_title(&SetTitle {
            thread: world.thread,
            title_job_id: job,
            text: "A title".to_owned(),
        })
        .await
        .expect("set");
    world.graphql.reply_with(reply(
        200,
        json!({ "data": { "threadsFailTitleJob": { "success": true } } }),
    ));
    world
        .client
        .fail_title_job(&FailTitleJob {
            thread: world.thread,
            title_job_id: job,
            code: "MODEL_FAILED".to_owned(),
            message: None,
        })
        .await
        .expect("failed");
    let bodies: Vec<Value> = world
        .graphql
        .bodies()
        .iter()
        .map(|b| serde_json::from_slice(b).expect("json"))
        .collect();
    assert_eq!(bodies[0]["variables"]["title"], "A title");
    assert_eq!(bodies[0]["variables"]["jobId"], json!(job));
    assert_eq!(bodies[1]["variables"]["reasonCode"], "MODEL_FAILED");
    assert_eq!(bodies[1]["variables"]["message"], Value::Null);
}

#[tokio::test]
async fn an_acknowledgement_that_is_not_a_success_is_unreadable() {
    let world = world();
    world.graphql.reply_with(reply(
        200,
        json!({ "data": { "threadsSetTitle": { "success": false } } }),
    ));
    let title = SetTitle {
        thread: world.thread,
        title_job_id: v7(),
        text: "t".to_owned(),
    };
    assert!(matches!(
        world.client.set_title(&title).await,
        Err(Error::Unreadable(_))
    ));
}

fn upload(world: &World) -> UploadRequest {
    UploadRequest {
        artifact_id: v7(),
        thread: world.thread,
        name: "report.pdf".to_owned(),
        media_type: "application/pdf".to_owned(),
        size: 1234,
        sha256: "ab".repeat(32),
    }
}

#[tokio::test]
async fn an_upload_request_gives_the_access_and_surfaces_a_refusal_as_it_is() {
    let world = world();
    world.graphql.reply_with(reply(
        200,
        json!({ "data": { "threadsRequestArtifactUpload": { "url": "https://s3/x", "fields": { "key": "k" } } } }),
    ));
    let request = upload(&world);
    let access = world.client.request_upload(&request).await.expect("access");
    assert_eq!(access.url, "https://s3/x");
    assert_eq!(access.fields["key"], "k");
    let sent: Value = serde_json::from_slice(&world.graphql.bodies()[0]).expect("json");
    assert_eq!(sent["variables"]["size"], 1234);
    assert_eq!(sent["variables"]["artifactId"], json!(request.artifact_id));

    world.graphql.reply_with(reply(
        200,
        json!({ "errors": [coded("ARTIFACT_NOT_READY")] }),
    ));
    match world.client.request_upload(&upload(&world)).await {
        Err(Error::Refused(refusal)) => assert_eq!(refusal.code, "ARTIFACT_NOT_READY"),
        other => panic!("refused, not {other:?}"),
    }
}

#[tokio::test]
async fn an_upload_request_needs_a_v7_artifact_id_and_sends_nothing_otherwise() {
    let world = world();
    let mut request = upload(&world);
    request.artifact_id = not_v7();
    assert!(matches!(
        world.client.request_upload(&request).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(world.graphql.bodies().is_empty());
}
