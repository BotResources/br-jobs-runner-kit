use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::*;

async fn start(ready: bool) -> (std::net::SocketAddr, CancellationToken, Arc<Readiness>) {
    let readiness = Arc::new(Readiness::new(["ns".to_owned()]));
    if ready {
        readiness.namespace_synced("ns");
        readiness.store_ready();
    }
    let state = Arc::new(HttpState {
        readiness: readiness.clone(),
        metrics: Box::new(|| "# TYPE x gauge\nx 1\n".to_owned()),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    tokio::spawn(serve(listener, state, shutdown.clone()));
    (address, shutdown, readiness)
}

async fn request(address: std::net::SocketAddr, method: &str, path: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect(address).await.unwrap();
    let text = format!("{method} {path} HTTP/1.1\r\nHost: keeper\r\nConnection: close\r\n\r\n");
    stream.write_all(text.as_bytes()).await.unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await.unwrap();
    let (head, body) = answer.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, head.to_owned(), body.to_owned())
}

#[tokio::test]
async fn livez_answers_200_while_the_process_runs_even_when_not_ready() {
    let (address, shutdown, _) = start(false).await;
    assert_eq!(request(address, "GET", "/livez").await.0, 200);
    shutdown.cancel();
}

#[tokio::test]
async fn readyz_answers_503_until_ready_and_200_after() {
    let (address, shutdown, readiness) = start(false).await;
    assert_eq!(request(address, "GET", "/readyz").await.0, 503);
    readiness.namespace_synced("ns");
    assert_eq!(request(address, "GET", "/readyz").await.0, 503);
    readiness.store_ready();
    assert_eq!(request(address, "GET", "/readyz").await.0, 200);
    shutdown.cancel();
}

#[tokio::test]
async fn metrics_answers_the_text_format() {
    let (address, shutdown, _) = start(true).await;
    let (status, head, body) = request(address, "GET", "/metrics").await;
    assert_eq!(status, 200);
    assert!(head.to_ascii_lowercase().contains("text/plain; version=0.0.4"));
    assert_eq!(body, "# TYPE x gauge\nx 1\n");
    shutdown.cancel();
}

#[tokio::test]
async fn another_path_is_404_and_another_method_is_405() {
    let (address, shutdown, _) = start(true).await;
    assert_eq!(request(address, "GET", "/other").await.0, 404);
    assert_eq!(request(address, "POST", "/metrics").await.0, 405);
    assert_eq!(request(address, "DELETE", "/livez").await.0, 405);
    shutdown.cancel();
}

#[tokio::test]
async fn the_server_stops_at_the_shutdown_signal() {
    let readiness = Arc::new(Readiness::new([]));
    let state = Arc::new(HttpState { readiness, metrics: Box::new(String::new) });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(serve(listener, state, shutdown.clone()));
    shutdown.cancel();
    handle.await.unwrap();
}
