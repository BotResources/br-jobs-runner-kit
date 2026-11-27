use br_runner_sandbox::Store;
use bytes::Bytes;
use serde_json::json;

use crate::fakes::{HttpServer, Response};
use crate::s3_store::store_for;

async fn put_target(path_style: bool, trailing_slash: bool) -> String {
    let server = HttpServer::start(|_| Response::status(200).header("etag", "\"e1\"")).await;
    let endpoint = if trailing_slash {
        format!("{}/", server.address)
    } else {
        server.address.clone()
    };
    let store = store_for(&endpoint, json!({"store_path_style": path_style}));
    store
        .put("a/b.json", Bytes::from_static(b"x"))
        .await
        .expect("a put");
    server.requests().remove(0).target
}

#[tokio::test]
async fn local_http_the_virtual_host_style_puts_one_slash_between_the_host_and_the_key() {
    assert_eq!(put_target(false, false).await, "/a/b.json");
    assert_eq!(put_target(false, true).await, "/a/b.json");
}

#[tokio::test]
async fn local_http_the_path_style_puts_one_slash_before_the_bucket_and_one_before_the_key() {
    assert_eq!(put_target(true, false).await, "/saves/a/b.json");
    assert_eq!(put_target(true, true).await, "/saves/a/b.json");
}
