use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

use super::*;
use crate::testing;

mod secret;

struct Canned {
    status: u16,
    body: String,
}

struct Stub {
    address: SocketAddr,
    heads: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    async fn serving(responses: Vec<Canned>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let seen = heads.clone();
        tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut head = Vec::new();
                let mut chunk = [0u8; 1024];
                while !head.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut chunk).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    head.extend_from_slice(&chunk[..read]);
                }
                let text = String::from_utf8_lossy(&head).into_owned();
                seen.lock()
                    .unwrap()
                    .push(text.lines().next().unwrap_or_default().to_owned());
                let reply = format!(
                    "HTTP/1.1 {} X\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.status,
                    response.body.len(),
                    response.body
                );
                stream.write_all(reply.as_bytes()).await.unwrap();
                stream.shutdown().await.ok();
            }
        });
        Self { address, heads }
    }

    fn bucket(&self, path_style: bool) -> S3Bucket {
        let mut settings = testing::config().store;
        settings.endpoint = Url::parse(&format!("http://{}", self.address)).unwrap();
        settings.path_style = path_style;
        S3Bucket::new(&settings, "key", "secret").unwrap()
    }

    fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }
}

fn error_body(code: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>{code}</Code><Message>m</Message></Error>"
    )
}

fn listing(keys: &[&str]) -> String {
    let contents: String = keys
        .iter()
        .map(|key| {
            format!(
                "<Contents><Key>{key}</Key><LastModified>2026-01-01T00:00:00.000Z</LastModified><ETag>\"e\"</ETag><Size>1</Size><StorageClass>STANDARD</StorageClass></Contents>"
            )
        })
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Name>saves</Name><Prefix></Prefix><KeyCount>{}</KeyCount><MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated>{contents}</ListBucketResult>",
        keys.len()
    )
}

fn canned(status: u16, body: String) -> Canned {
    Canned { status, body }
}

#[tokio::test]
async fn a_store_that_answers_no_such_bucket_is_an_absent_bucket() {
    let stub = Stub::serving(vec![canned(404, error_body("NoSuchBucket"))]).await;
    assert_eq!(stub.bucket(true).check().await, Err(BucketError::Absent));
}

#[tokio::test]
async fn a_store_that_refuses_the_credential_is_unreachable_and_not_absent() {
    let stub = Stub::serving(vec![canned(403, error_body("AccessDenied"))]).await;
    assert!(matches!(
        stub.bucket(true).check().await,
        Err(BucketError::Unreachable(_))
    ));
}

#[tokio::test]
async fn a_store_that_lists_and_a_store_with_no_object_are_both_present() {
    let stub = Stub::serving(vec![
        canned(200, listing(&[])),
        canned(200, listing(&["a"])),
    ])
    .await;
    let bucket = stub.bucket(true);
    assert_eq!(bucket.check().await, Ok(()));
    assert_eq!(bucket.check().await, Ok(()));
}

#[tokio::test]
async fn the_start_check_lists_only_under_a_prefix_that_the_keeper_uses() {
    let stub = Stub::serving(vec![canned(200, listing(&["saves/a"]))]).await;
    assert_eq!(stub.bucket(true).check().await, Ok(()));
    let heads = stub.heads();
    assert_eq!(heads.len(), 1);
    assert!(heads[0].contains("prefix=saves%2F"), "{heads:?}");
}

#[tokio::test]
async fn a_page_takes_at_most_its_limit_and_the_next_page_starts_after_the_last_key() {
    let keys = [
        "saves/c/t/a/u1/manifest.json",
        "saves/c/t/a/u2/manifest.json",
        "saves/c/t/a/u3/manifest.json",
    ];
    let stub = Stub::serving(vec![
        canned(200, listing(&keys)),
        canned(200, listing(&keys[2..])),
    ])
    .await;
    let bucket = stub.bucket(true);
    let first = bucket.list_page("saves/", None, 2).await.unwrap();
    assert_eq!(
        first
            .iter()
            .map(|info| info.key.as_str())
            .collect::<Vec<_>>(),
        keys[..2].to_vec()
    );
    let next = bucket
        .list_page("saves/", Some(&first[1].key), 2)
        .await
        .unwrap();
    assert_eq!(next.len(), 1);
    let heads = stub.heads();
    assert!(heads[0].contains("prefix=saves%2F"), "{heads:?}");
    assert!(!heads[0].contains("start-after"), "{heads:?}");
    assert!(
        heads[1].contains("start-after=saves%2Fc%2Ft%2Fa%2Fu2%2Fmanifest.json"),
        "{heads:?}"
    );
}

#[tokio::test]
async fn an_absent_object_is_none_for_a_get_and_a_success_for_a_delete() {
    let stub = Stub::serving(vec![
        canned(404, error_body("NoSuchKey")),
        canned(404, error_body("NoSuchKey")),
    ])
    .await;
    let bucket = stub.bucket(true);
    assert_eq!(bucket.get("saves/x").await, Ok(None));
    assert_eq!(bucket.delete("saves/x").await, Ok(()));
}

#[tokio::test]
async fn the_path_style_setting_puts_the_bucket_name_in_the_path() {
    let path_style = Stub::serving(vec![canned(200, listing(&[]))]).await;
    path_style.bucket(true).check().await.unwrap();
    assert!(
        path_style.heads()[0].starts_with("GET /saves?"),
        "{:?}",
        path_style.heads()
    );
}

#[test]
fn a_key_that_is_not_a_valid_path_is_refused_before_any_call() {
    assert!(matches!(path("saves//x"), Err(BucketError::Call(_))));
    assert!(path("saves/c/t/a/u/manifest.json").is_ok());
}
