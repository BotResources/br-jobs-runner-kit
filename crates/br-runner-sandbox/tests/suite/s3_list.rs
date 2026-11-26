use br_runner_sandbox::{Store, StoreError};
use serde_json::json;

use crate::fakes::{HttpServer, Response};
use crate::s3_store::store_for;
use crate::s3_streams::param;

fn listing(keys: &[&str], next: Option<&str>) -> Response {
    let contents: String = keys
        .iter()
        .map(|key| {
            format!(
                "<Contents><Key>{key}</Key><Size>3</Size><LastModified>2026-11-26T10:00:00.000Z</LastModified></Contents>"
            )
        })
        .collect();
    let token = next
        .map(|token| format!("<NextContinuationToken>{token}</NextContinuationToken>"))
        .unwrap_or_default();
    Response::body(
        200,
        format!(
            "<ListBucketResult><IsTruncated>{}</IsTruncated>{contents}{token}</ListBucketResult>",
            next.is_some()
        ),
    )
}

#[tokio::test]
async fn local_http_a_list_follows_every_page_and_returns_each_key_once_as_a_full_key() {
    let server = HttpServer::start(|request| match param(request, "continuation-token") {
        None => listing(&["t/1/a.part", "t/1/b.part"], Some("T2")),
        Some("T2") => listing(&["t/1/c.part"], None),
        Some(_) => Response::status(400),
    })
    .await;
    let store = store_for(&server.address, json!({}));
    let keys = store.list("t/1/").await.expect("a listing");
    assert_eq!(keys, vec!["t/1/a.part", "t/1/b.part", "t/1/c.part"]);
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.method, "GET");
        assert_eq!(param(request, "list-type"), Some("2"));
        assert_eq!(param(request, "prefix"), Some("t%2F1%2F"));
    }
    assert_eq!(param(&requests[1], "continuation-token"), Some("T2"));
}

#[tokio::test]
async fn local_http_the_keys_of_a_list_are_the_keys_a_delete_receives() {
    let server = HttpServer::start(|request| match request.method.as_str() {
        "GET" => listing(&["t/1/a.part", "t/1/b.part"], None),
        _ => Response::status(204),
    })
    .await;
    let store = store_for(&server.address, json!({}));
    for key in store.list("t/1/").await.expect("a listing") {
        store.delete(&key).await.expect("a delete");
    }
    let deletes: Vec<String> = server
        .requests()
        .into_iter()
        .filter(|request| request.method == "DELETE")
        .map(|request| request.path().to_owned())
        .collect();
    assert_eq!(
        deletes,
        vec![
            "/saves/t/1/a.part".to_owned(),
            "/saves/t/1/b.part".to_owned()
        ]
    );
}

#[tokio::test]
async fn local_http_a_list_of_a_prefix_with_no_object_is_empty() {
    let server = HttpServer::start(|_| listing(&[], None)).await;
    let store = store_for(&server.address, json!({}));
    assert_eq!(store.list("t/9/").await, Ok(Vec::new()));
}

#[tokio::test]
async fn local_http_a_list_refused_by_the_store_is_a_call_error() {
    let server =
        HttpServer::start(|_| Response::body(403, "<Error><Code>AccessDenied</Code></Error>"))
            .await;
    let store = store_for(&server.address, json!({}));
    assert!(matches!(store.list("t/1/").await, Err(StoreError::Call(_))));
}

#[tokio::test]
async fn local_http_a_prefix_that_is_not_a_valid_object_path_is_a_key_error_before_any_request() {
    let server = HttpServer::start(|_| listing(&[], None)).await;
    let store = store_for(&server.address, json!({}));
    assert_eq!(
        store.list("a//b/").await,
        Err(StoreError::Key("a//b".to_owned()))
    );
    assert!(server.requests().is_empty());
}
