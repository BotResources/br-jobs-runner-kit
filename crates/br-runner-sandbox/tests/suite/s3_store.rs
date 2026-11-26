use br_runner_sandbox::{PutOutcome, S3Store, Store, StoreError};
use bytes::Bytes;
use serde_json::json;

use crate::common;
use crate::fakes::{HttpServer, Request, Response};

const SECRET: &str = "wJalrSECRETEXAMPLE";

pub fn store_for(address: &str, extra: serde_json::Value) -> S3Store {
    let mut settings = json!({"store_endpoint": address, "store_path_style": true});
    if let (Some(base), Some(extra)) = (settings.as_object_mut(), extra.as_object()) {
        base.extend(extra.clone());
    }
    let config = common::build(common::with(settings)).expect("a configuration");
    S3Store::new(config.store()).expect("a store")
}

fn is_put(request: &Request) -> bool {
    request.method == "PUT"
}

#[tokio::test]
async fn local_http_a_conditional_create_asks_for_an_absent_object_and_a_precondition_failure_is_already_exists()
 {
    let server = HttpServer::start(|request| {
        if request.headers.contains_key("if-none-match") && request.path().ends_with("/taken") {
            Response::status(412)
        } else {
            Response::status(200).header("etag", "\"e1\"")
        }
    })
    .await;
    let store = store_for(&server.address, json!({}));
    assert_eq!(
        store
            .put_if_absent("fresh", Bytes::from_static(b"{}"))
            .await,
        Ok(PutOutcome::Created)
    );
    assert_eq!(
        store
            .put_if_absent("taken", Bytes::from_static(b"{}"))
            .await,
        Ok(PutOutcome::AlreadyExists)
    );
    let puts: Vec<Request> = server.requests().into_iter().filter(is_put).collect();
    assert_eq!(puts.len(), 2);
    for put in &puts {
        assert_eq!(
            put.headers.get("if-none-match").map(String::as_str),
            Some("*")
        );
        assert_eq!(put.body, b"{}");
    }
}

#[tokio::test]
async fn local_http_the_path_style_puts_the_bucket_in_the_path() {
    let server = HttpServer::start(|_| Response::status(200).header("etag", "\"e1\"")).await;
    let path_style = store_for(&server.address, json!({}));
    path_style
        .put("a/b.json", Bytes::from_static(b"x"))
        .await
        .expect("a put");
    let first = server.requests().remove(0);
    assert_eq!(first.path(), "/saves/a/b.json");
    assert!(first.headers["host"].starts_with("127.0.0.1"));
}

#[tokio::test]
async fn local_http_a_request_is_signed_and_the_secret_is_never_sent() {
    let server = HttpServer::start(|_| Response::status(200).header("etag", "\"e1\"")).await;
    let store = store_for(&server.address, json!({}));
    store
        .put("k", Bytes::from_static(b"x"))
        .await
        .expect("a put");
    let request = server.requests().remove(0);
    let authorization = request.headers.get("authorization").expect("a signature");
    assert!(authorization.starts_with("AWS4-HMAC-SHA256"));
    assert!(authorization.contains("AKIDEXAMPLE"));
    assert!(authorization.contains("eu-west-1"));
    assert!(!format!("{request:?}").contains(SECRET));
}

#[tokio::test]
async fn local_http_a_missing_object_is_none_and_a_delete_of_a_missing_object_is_ok() {
    let server = HttpServer::start(|request| match request.method.as_str() {
        "GET" | "HEAD" | "DELETE" => Response::body(404, "<Error><Code>NoSuchKey</Code></Error>"),
        _ => Response::status(200),
    })
    .await;
    let store = store_for(&server.address, json!({}));
    assert_eq!(store.get("k").await, Ok(None));
    assert_eq!(store.size("k").await, Ok(None));
    assert_eq!(store.delete("k").await, Ok(()));
    assert!(matches!(store.get_stream("k").await, Ok(None)));
}

#[tokio::test]
async fn local_http_a_get_returns_the_body_and_a_head_returns_the_size() {
    let server = HttpServer::start(|request| {
        let base = Response::status(200)
            .header("etag", "\"e1\"")
            .header("last-modified", "Thu, 26 Nov 2026 10:00:00 GMT");
        match request.method.as_str() {
            "HEAD" => base.header("content-length", "5"),
            _ => Response::body(200, "hello")
                .header("etag", "\"e1\"")
                .header("last-modified", "Thu, 26 Nov 2026 10:00:00 GMT"),
        }
    })
    .await;
    let store = store_for(&server.address, json!({}));
    assert_eq!(store.get("k").await, Ok(Some(Bytes::from_static(b"hello"))));
    assert_eq!(store.size("k").await, Ok(Some(5)));
}

#[tokio::test]
async fn local_http_a_server_error_is_a_call_error_that_does_not_hold_the_secret() {
    let server = HttpServer::start(|_| Response::body(500, "boom")).await;
    let store = store_for(&server.address, json!({}));
    let error = store.get("k").await.expect_err("an error");
    assert!(matches!(error, StoreError::Call(_)));
    assert!(!format!("{error} {error:?}").contains(SECRET));
}

#[tokio::test]
async fn local_http_a_key_that_is_not_a_valid_object_path_is_a_key_error_before_any_request() {
    let server = HttpServer::start(|_| Response::status(200)).await;
    let store = store_for(&server.address, json!({}));
    assert_eq!(
        store.get("a//b").await,
        Err(StoreError::Key("a//b".to_owned()))
    );
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn the_debug_text_of_a_store_holds_no_credentials() {
    let store = store_for("http://127.0.0.1:1", json!({}));
    let text = format!("{store:?}");
    assert!(!text.contains(SECRET));
    assert!(!text.contains("AKIDEXAMPLE"));
}
