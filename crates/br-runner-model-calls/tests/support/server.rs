//! A local HTTP server that replays a recorded stream for every request.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone, Debug)]
pub enum Step {
    /// One SSE frame whose data is this JSON.
    Frame(Value),
    Pause(Duration),
}

#[derive(Clone, Debug)]
pub enum Reply {
    Stream(Vec<Step>),
    Status(u16, String),
}

pub struct TestServer {
    addr: SocketAddr,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl TestServer {
    pub async fn start(reply: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let requests = Arc::new(AtomicUsize::new(0));
        let counted = requests.clone();
        let reply = Arc::new(reply);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serve(stream, reply.clone()));
            }
        });
        Self {
            addr,
            requests,
            task,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}/v1/", self.addr)
    }

    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).to_ascii_lowercase();
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len() < head_end + length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Some(())
}

async fn serve(mut stream: TcpStream, reply: Arc<Reply>) {
    if read_request(&mut stream).await.is_none() {
        return;
    }
    match &*reply {
        Reply::Status(status, body) => {
            let head = format!(
                "HTTP/1.1 {status} Refused\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(body.as_bytes()).await;
        }
        Reply::Stream(steps) => {
            let head =
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
            if stream.write_all(head.as_bytes()).await.is_err() {
                return;
            }
            for step in steps {
                match step {
                    Step::Frame(data) => {
                        let frame = format!("data: {data}\n\n");
                        if stream.write_all(frame.as_bytes()).await.is_err() {
                            return;
                        }
                        let _ = stream.flush().await;
                    }
                    Step::Pause(pause) => tokio::time::sleep(*pause).await,
                }
            }
        }
    }
    let _ = stream.shutdown().await;
}
