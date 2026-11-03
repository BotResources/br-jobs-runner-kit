mod support;

use br_runner_threads::test_double;
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{Cause, Error, SetTitle, UploadAccess, UploadRequest};
use serde_json::json;
use support::client_graphql_x::*;
use support::client_x::*;
use support::*;

fn set_title() -> SetTitle {
    SetTitle {
        thread_id: v7(),
        text: "A fine title".to_owned(),
    }
}

fn upload_request() -> UploadRequest {
    UploadRequest {
        artifact_id: v7(),
        thread_id: v7(),
        name: "report.pdf".to_owned(),
        media_type: "application/pdf".to_owned(),
        size: 2048,
        sha256: "ab".repeat(32),
    }
}

#[tokio::test]
async fn setting_the_title_sends_the_thread_and_the_text() {
    let threads = MemoryThreads::new();
    answering(&threads, json!({ "threadsSetTitle": { "success": true } }));
    let title = set_title();

    threads
        .client()
        .set_title(&title)
        .await
        .expect("acknowledged");

    let sent = graphql_operations(&threads);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].0.contains("threadsSetTitle"));
    assert_eq!(
        sent[0].1,
        json!({
            "threadId": title.thread_id,
            "title": "A fine title",
        })
    );
}

#[tokio::test]
async fn a_title_that_is_not_acknowledged_is_unreadable() {
    let unacknowledged = [
        json!({}),
        json!({ "threadsSetTitle": null }),
        json!({ "threadsSetTitle": { "success": false } }),
        json!({ "threadsSetTitle": { "success": "yes" } }),
        json!({ "threadsCreateThread": { "success": true } }),
    ];
    for answer in unacknowledged {
        let threads = MemoryThreads::new();
        answering(&threads, answer.clone());

        let result = threads.client().set_title(&set_title()).await;

        assert!(
            matches!(result, Err(Error::Unreadable(_))),
            "{answer}: {result:?}"
        );
    }
}

#[tokio::test]
async fn requesting_an_upload_sends_the_file_values_and_gives_the_access_as_threads_gave_it() {
    let threads = MemoryThreads::new();
    answering(
        &threads,
        json!({ "threadsRequestArtifactUpload": {
            "url": "https://files.example/upload",
            "fields": { "key": "abc", "policy": "xyz" },
        } }),
    );
    let request = upload_request();

    let access = threads
        .client()
        .request_upload(&request)
        .await
        .expect("access");

    let sent = graphql_operations(&threads);
    assert!(sent[0].0.contains("threadsRequestArtifactUpload"));
    assert_eq!(
        sent[0].1,
        json!({
            "threadId": request.thread_id,
            "artifactId": request.artifact_id,
            "name": "report.pdf",
            "mediaType": "application/pdf",
            "size": 2048,
            "sha256": request.sha256,
        })
    );
    let UploadAccess { url, fields } = access;
    assert_eq!(url, "https://files.example/upload");
    assert_eq!(fields["key"], json!("abc"));
    assert_eq!(fields["policy"], json!("xyz"));
    assert_eq!(fields.len(), 2);
}

#[tokio::test]
async fn an_artifact_id_that_is_not_a_uuidv7_is_refused_before_the_upload_is_requested() {
    let threads = MemoryThreads::new();
    let mut request = upload_request();
    request.artifact_id = not_v7();

    let result = threads.client().request_upload(&request).await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert!(threads.operations().is_empty());
}

#[tokio::test]
async fn an_upload_access_that_is_missing_or_malformed_is_unreadable() {
    let unreadable = [
        json!({}),
        json!({ "threadsRequestArtifactUpload": null }),
        json!({ "threadsRequestArtifactUpload": { "fields": {} } }),
        json!({ "threadsRequestArtifactUpload": { "url": "https://x", "fields": [1] } }),
        json!({ "threadsRequestArtifactUpload": { "url": 7, "fields": {} } }),
    ];
    for answer in unreadable {
        let threads = MemoryThreads::new();
        answering(&threads, answer.clone());

        let result = threads.client().request_upload(&upload_request()).await;

        assert!(
            matches!(result, Err(Error::Unreadable(_))),
            "{answer}: {result:?}"
        );
    }
}

#[tokio::test]
async fn a_failure_of_the_graphql_request_reaches_the_caller_for_every_operation() {
    let threads = MemoryThreads::new();
    let failure = Error::Lost(test_double::failure(Cause::WaitFired, "no reply"));
    let expected = failure.clone();
    threads.answer_graphql(move |_, _| Err(failure.clone()));
    let client = threads.client();

    let title = client.set_title(&set_title()).await;
    let upload = client.request_upload(&upload_request()).await;

    assert_eq!(title, Err(expected.clone()));
    assert_eq!(upload.expect_err("failed"), expected);
}
