mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use br_agentic_contract::ThreadRef;
use br_llm_clients_common_auth::{AccessToken, TokenCache, TokenSource};
use br_llm_clients_common_http::{
    AuthorizerError, BoxFuture, Endpoint, HttpClient, HttpConfig, Secret,
};
use br_runner_threads::transport::{GraphqlTransport, HttpGraphql, MemoryBus};
use br_runner_threads::{ContextQuery, Error, ThreadsClient};
use support::{actor, v7};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "tok-0123456789-secret";

struct Counting {
    fetched: Arc<AtomicUsize>,
    fails: bool,
}

impl TokenSource for Counting {
    fn fetch(&self) -> BoxFuture<'_, Result<AccessToken, AuthorizerError>> {
        Box::pin(async move {
            self.fetched.fetch_add(1, Ordering::SeqCst);
            if self.fails {
                return Err(AuthorizerError::fetch_failed(format!(
                    "identity said no to {TOKEN}"
                )));
            }
            Ok(AccessToken::new(Secret::new(TOKEN).expect("secret"), None))
        })
    }

    fn secrets(&self) -> Vec<Secret> {
        vec![Secret::new(TOKEN).expect("secret")]
    }
}

fn transport(port: u16, fails: bool) -> (HttpGraphql, Arc<AtomicUsize>) {
    let fetched = Arc::new(AtomicUsize::new(0));
    let source = Counting {
        fetched: Arc::clone(&fetched),
        fails,
    };
    let config = HttpConfig::new(
        Duration::from_millis(500),
        Duration::from_millis(1500),
        Duration::from_millis(1500),
    );
    let client = HttpClient::new(config).expect("client");
    let endpoint =
        Endpoint::plain_http(&format!("http://127.0.0.1:{port}/graphql")).expect("endpoint");
    (
        HttpGraphql::new(endpoint, client, TokenCache::new(Arc::new(source))),
        fetched,
    )
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().expect("length"))
        })
        .unwrap_or(0)
}

async fn serve_once(response: Vec<u8>) -> (u16, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut received = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let read = socket.read(&mut buffer).await.expect("read");
            received.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&received).to_string();
            if let Some(split) = text.find("\r\n\r\n")
                && received.len() >= split + 4 + content_length(&text[..split])
            {
                break;
            }
            if read == 0 {
                break;
            }
        }
        socket.write_all(&response).await.expect("write");
        socket.flush().await.expect("flush");
        drop(socket);
        String::from_utf8_lossy(&received).to_string()
    });
    (port, task)
}

fn http(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[tokio::test]
async fn the_bearer_token_and_the_body_reach_the_server_and_the_reply_comes_back() {
    let (port, server) = serve_once(http("200 OK", "{\"data\":{}}")).await;
    let (graphql, fetched) = transport(port, false);
    let reply = graphql
        .post(b"{\"query\":\"q\"}".to_vec())
        .await
        .expect("reply");
    assert_eq!(
        (reply.status, reply.body.as_slice()),
        (200, b"{\"data\":{}}".as_slice())
    );
    let received = server.await.expect("server").to_ascii_lowercase();
    assert!(received.contains(&format!("authorization: bearer {TOKEN}")));
    assert!(received.contains("content-type: application/json"));
    assert!(received.ends_with("{\"query\":\"q\"}"));
    assert_eq!(fetched.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn nobody_listening_is_unavailable() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let (graphql, _) = transport(port, false);
    assert!(matches!(
        graphql.post(Vec::new()).await,
        Err(Error::Unavailable(_))
    ));
}

#[tokio::test]
async fn a_body_cut_short_after_the_request_left_is_an_answer_lost() {
    let cut = b"HTTP/1.1 200 OK\r\ncontent-length: 500\r\nconnection: close\r\n\r\n{\"da".to_vec();
    let (port, _server) = serve_once(cut).await;
    let (graphql, _) = transport(port, false);
    assert!(matches!(
        graphql.post(b"{}".to_vec()).await,
        Err(Error::AnswerLost(_))
    ));
}

#[tokio::test]
async fn a_token_source_that_fails_is_an_auth_error_that_hides_the_secrets_it_declares() {
    let (graphql, fetched) = transport(1, true);
    match graphql.post(Vec::new()).await {
        Err(Error::Auth(text)) => assert!(!text.contains(TOKEN), "{text}"),
        other => panic!("auth, not {other:?}"),
    }
    assert_eq!(fetched.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_401_is_returned_as_a_reply_and_the_token_is_not_fetched_a_second_time() {
    let (port, _server) = serve_once(http("401 Unauthorized", "denied")).await;
    let (graphql, fetched) = transport(port, false);
    let reply = graphql.post(b"{}".to_vec()).await.expect("a reply");
    assert_eq!(reply.status, 401);
    assert_eq!(fetched.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn through_the_client_a_401_is_unauthenticated_and_a_500_never_echoes_the_token() {
    for (status, body) in [
        ("401 Unauthorized", "denied"),
        ("500 Internal Server Error", TOKEN),
    ] {
        let (port, _server) = serve_once(http(status, body)).await;
        let (graphql, _) = transport(port, false);
        let client = ThreadsClient::new(actor(), Arc::new(MemoryBus::new()), Arc::new(graphql));
        let query = ContextQuery {
            thread: ThreadRef::new(v7()),
            agent: None,
        };
        let error = client.read_context(&query).await.expect_err("an error");
        assert!(!format!("{error:?} {error}").contains(TOKEN));
        if status.starts_with("401") {
            assert_eq!(error, Error::Unauthenticated);
        } else {
            assert!(matches!(error, Error::AnswerLost(_)));
        }
    }
}
