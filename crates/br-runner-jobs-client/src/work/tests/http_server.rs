use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

#[derive(Debug, Clone)]
pub struct Recorded {
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

    pub fn header_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.headers.iter().map(|(key, _)| key.as_str()).collect();
        names.sort_unstable();
        names
    }
}

pub struct Action {
    status: u16,
    body: Vec<u8>,
}

impl Action {
    pub fn json(status: u16, body: serde_json::Value) -> Self {
        Self::reply(status, serde_json::to_vec(&body).expect("json"))
    }

    pub fn reply(status: u16, body: Vec<u8>) -> Self {
        Self { status, body }
    }
}

pub type Handler = Arc<dyn Fn(&Recorded) -> Action + Send + Sync>;

pub fn serve(handler: impl Fn(&Recorded) -> Action + Send + Sync + 'static) -> Handler {
    Arc::new(handler)
}

pub struct HttpServer {
    port: u16,
    recorded: Arc<Mutex<Vec<Recorded>>>,
    task: JoinHandle<()>,
}

impl HttpServer {
    pub async fn start(handler: Handler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let task = tokio::spawn(accept(listener, handler, Arc::clone(&recorded)));
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

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn accept(listener: TcpListener, handler: Handler, recorded: Arc<Mutex<Vec<Recorded>>>) {
    let mut connections = JoinSet::new();
    while let Ok((stream, _)) = listener.accept().await {
        connections.spawn(connection(
            stream,
            Arc::clone(&handler),
            Arc::clone(&recorded),
        ));
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut buffer = Vec::new();
    let head_end = loop {
        if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return None,
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let headers: Vec<(String, String)> = head
        .split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return None,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    Some(Recorded { headers, body })
}

fn response(action: &Action) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
        action.status,
        action.body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(&action.body);
    bytes
}

async fn connection(mut stream: TcpStream, handler: Handler, recorded: Arc<Mutex<Vec<Recorded>>>) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    let action = handler(&request);
    recorded.lock().expect("lock").push(request);
    let _ = stream.write_all(&response(&action)).await;
    let mut chunk = [0u8; 256];
    while matches!(stream.read(&mut chunk).await, Ok(n) if n > 0) {}
}
