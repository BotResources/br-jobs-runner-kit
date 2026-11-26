use br_runner_sandbox::{Store, StoreError};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::json;

use crate::fakes::{HttpServer, Request, Response};
use crate::s3_store::store_for;

const MEBIBYTE: usize = 1024 * 1024;

fn query(request: &Request) -> &str {
    request
        .target
        .split_once('?')
        .map_or("", |(_, query)| query)
}

fn has_param(request: &Request, name: &str) -> bool {
    query(request)
        .split('&')
        .any(|pair| pair.split('=').next() == Some(name))
}

pub fn param<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    query(request)
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

fn is_start(request: &Request) -> bool {
    request.method == "POST" && has_param(request, "uploads")
}

fn is_part(request: &Request) -> bool {
    request.method == "PUT" && has_param(request, "partNumber")
}

fn is_complete(request: &Request) -> bool {
    request.method == "POST" && has_param(request, "uploadId")
}

fn is_abort(request: &Request) -> bool {
    request.method == "DELETE" && has_param(request, "uploadId")
}

fn multipart_answer(request: &Request) -> Response {
    if is_start(request) {
        return Response::body(
            200,
            "<InitiateMultipartUploadResult><UploadId>U1</UploadId></InitiateMultipartUploadResult>",
        );
    }
    if is_part(request) {
        let number = param(request, "partNumber").unwrap_or("0");
        return Response::status(200).header("etag", &format!("\"part-{number}\""));
    }
    if is_complete(request) {
        return Response::body(
            200,
            "<CompleteMultipartUploadResult><ETag>\"whole\"</ETag></CompleteMultipartUploadResult>",
        );
    }
    Response::status(204)
}

fn pattern(length: usize) -> Vec<u8> {
    (0..length).map(|index| (index % 251) as u8).collect()
}

fn sorted_parts(server: &HttpServer) -> Vec<Request> {
    let mut parts: Vec<Request> = server.requests().into_iter().filter(is_part).collect();
    parts.sort_by_key(|part| {
        param(part, "partNumber")
            .and_then(|number| number.parse::<u32>().ok())
            .unwrap_or(0)
    });
    parts
}

#[tokio::test]
async fn local_http_a_streamed_upload_sends_every_chunk_as_a_part_and_finish_completes_one_object()
{
    let server = HttpServer::start(multipart_answer).await;
    let store = store_for(&server.address, json!({}));
    let data = pattern(11 * MEBIBYTE);
    let mut upload = store.start_upload("up/part").await.expect("an upload");
    for chunk in data.chunks(MEBIBYTE) {
        upload
            .write(Bytes::copy_from_slice(chunk))
            .await
            .expect("a chunk");
    }
    upload.finish().await.expect("a finish");
    let parts = sorted_parts(&server);
    assert!(parts.len() >= 2);
    let sent: Vec<u8> = parts.iter().flat_map(|part| part.body.clone()).collect();
    assert_eq!(sent.len(), data.len());
    assert_eq!(sent, data);
    let requests = server.requests();
    assert_eq!(requests.iter().filter(|r| is_start(r)).count(), 1);
    assert!(
        requests
            .iter()
            .all(|request| request.path().ends_with("/saves/up/part"))
    );
    let completes: Vec<&Request> = requests.iter().filter(|r| is_complete(r)).collect();
    assert_eq!(completes.len(), 1);
    assert_eq!(param(completes[0], "uploadId"), Some("U1"));
    let body = String::from_utf8_lossy(&completes[0].body).into_owned();
    assert_eq!(body.matches("<Part>").count(), parts.len());
    assert!(body.contains("part-"));
    assert!(!requests.iter().any(is_abort));
}

#[tokio::test]
async fn local_http_a_streamed_upload_of_less_than_one_part_is_sent_as_one_part_at_finish() {
    let server = HttpServer::start(multipart_answer).await;
    let store = store_for(&server.address, json!({}));
    let mut upload = store.start_upload("k").await.expect("an upload");
    upload
        .write(Bytes::from_static(b"abc"))
        .await
        .expect("a chunk");
    upload
        .write(Bytes::from_static(b"def"))
        .await
        .expect("a chunk");
    assert!(server.requests().iter().all(|request| !is_part(request)));
    upload.finish().await.expect("a finish");
    let parts = sorted_parts(&server);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].body, b"abcdef");
}

#[tokio::test]
async fn local_http_an_aborted_upload_sends_the_abort_request_and_never_completes() {
    let server = HttpServer::start(multipart_answer).await;
    let store = store_for(&server.address, json!({}));
    let mut upload = store.start_upload("k").await.expect("an upload");
    upload
        .write(Bytes::from(pattern(6 * MEBIBYTE)))
        .await
        .expect("a chunk");
    upload.abort().await.expect("an abort");
    let requests = server.requests();
    let aborts: Vec<&Request> = requests.iter().filter(|r| is_abort(r)).collect();
    assert_eq!(aborts.len(), 1);
    assert_eq!(param(aborts[0], "uploadId"), Some("U1"));
    assert!(!requests.iter().any(is_complete));
}

#[tokio::test]
async fn local_http_a_refused_start_of_an_upload_is_a_call_error_without_a_part() {
    let server =
        HttpServer::start(|_| Response::body(403, "<Error><Code>AccessDenied</Code></Error>"))
            .await;
    let store = store_for(&server.address, json!({}));
    let error = store.start_upload("k").await.err().expect("an error");
    assert!(matches!(error, StoreError::Call(_)));
    assert!(server.requests().iter().all(|request| !is_part(request)));
}

#[tokio::test]
async fn local_http_a_key_that_is_not_a_valid_object_path_cannot_start_an_upload() {
    let server = HttpServer::start(multipart_answer).await;
    let store = store_for(&server.address, json!({}));
    assert!(matches!(
        store.start_upload("a//b").await,
        Err(StoreError::Key(key)) if key == "a//b"
    ));
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn local_http_a_streamed_get_yields_the_whole_body_in_order() {
    let data = pattern(3 * MEBIBYTE + 17);
    let served = data.clone();
    let server = HttpServer::start(move |_| {
        Response::body(200, served.clone())
            .header("etag", "\"e1\"")
            .header("last-modified", "Thu, 26 Nov 2026 10:00:00 GMT")
    })
    .await;
    let store = store_for(&server.address, json!({}));
    let mut stream = store
        .get_stream("k")
        .await
        .expect("a stream")
        .expect("an object");
    let mut received = Vec::new();
    while let Some(chunk) = stream.next().await {
        received.extend_from_slice(&chunk.expect("a chunk"));
    }
    assert_eq!(received, data);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path(), "/saves/k");
}

#[tokio::test]
async fn local_http_a_streamed_get_of_a_server_error_is_a_call_error() {
    let server = HttpServer::start(|_| Response::body(500, "boom")).await;
    let store = store_for(&server.address, json!({}));
    assert!(matches!(
        store.get_stream("k").await,
        Err(StoreError::Call(_))
    ));
}
