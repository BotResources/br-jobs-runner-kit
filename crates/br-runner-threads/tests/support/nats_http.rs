use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("the body is JSON")
    }
}

pub enum Answer {
    Json(u16, Value),
    Text(u16, String),
    Hang,
}

type Handler = Arc<dyn Fn(&Recorded) -> Answer + Send + Sync>;

pub struct HttpStandIn {
    port: u16,
    recorded: Arc<Mutex<Vec<Recorded>>>,
    task: tokio::task::JoinHandle<()>,
}

impl HttpStandIn {
    pub async fn start(handler: impl Fn(&Recorded) -> Answer + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let port = listener.local_addr().expect("its address").port();
        let recorded: Arc<Mutex<Vec<Recorded>>> = Arc::default();
        let handler: Handler = Arc::new(handler);
        let task = tokio::spawn({
            let recorded = recorded.clone();
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(serve(stream, handler.clone(), recorded.clone()));
                }
            }
        });
        Self {
            port,
            recorded,
            task,
        }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/graphql", self.port)
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.recorded.lock().expect("lock").clone()
    }
}

impl Drop for HttpStandIn {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(mut stream: TcpStream, handler: Handler, recorded: Arc<Mutex<Vec<Recorded>>>) {
    while let Some(request) = read_request(&mut stream).await {
        let answer = handler(&request);
        recorded.lock().expect("lock").push(request);
        let bytes = match answer {
            Answer::Json(status, body) => response(status, body.to_string().as_bytes()),
            Answer::Text(status, body) => response(status, body.as_bytes()),
            Answer::Hang => {
                std::future::pending::<()>().await;
                return;
            }
        };
        if stream.write_all(&bytes).await.is_err() {
            return;
        }
    }
}

async fn fill(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> bool {
    let mut chunk = [0u8; 4096];
    match stream.read(&mut chunk).await {
        Ok(0) | Err(_) => false,
        Ok(read) => {
            buffer.extend_from_slice(&chunk[..read]);
            true
        }
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut buffer = Vec::new();
    let head_end = loop {
        if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        if !fill(stream, &mut buffer).await {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(key, _)| key == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        if !fill(stream, &mut body).await {
            return None;
        }
    }
    Some(Recorded {
        method,
        target,
        headers,
        body,
    })
}

fn response(status: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}
