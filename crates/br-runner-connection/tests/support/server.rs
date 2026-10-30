use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

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

    pub fn header_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.headers.iter().map(|(key, _)| key.as_str()).collect();
        names.sort_unstable();
        names
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("the body is JSON")
    }
}

pub enum Action {
    Reply(u16, Vec<u8>),
    Raw(Vec<u8>),
    RawThenClose(Vec<u8>),
    Close,
    Hang,
}

impl Action {
    pub fn json(status: u16, body: serde_json::Value) -> Self {
        Self::Reply(status, serde_json::to_vec(&body).expect("json"))
    }
}

pub type Handler = Arc<dyn Fn(&Recorded) -> Action + Send + Sync>;

pub enum Mode {
    Serve(Handler),
    Mute,
    Drop,
}

#[derive(Default)]
struct Counters {
    accepted: AtomicUsize,
    requests: AtomicUsize,
    closed: AtomicUsize,
    recorded: Mutex<Vec<Recorded>>,
}

pub struct HttpServer {
    port: u16,
    counters: Arc<Counters>,
    task: JoinHandle<()>,
}

pub fn serve(handler: impl Fn(&Recorded) -> Action + Send + Sync + 'static) -> Mode {
    Mode::Serve(Arc::new(handler))
}

impl HttpServer {
    pub async fn start(mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let counters = Arc::new(Counters::default());
        let task = tokio::spawn(accept(listener, mode, Arc::clone(&counters)));
        Self {
            port,
            counters,
            task,
        }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/graphql", self.port)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn accepted(&self) -> usize {
        self.counters.accepted.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.counters.recorded.lock().expect("lock").clone()
    }

    pub fn closed(&self) -> usize {
        self.counters.closed.load(Ordering::SeqCst)
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn accept(listener: TcpListener, mode: Mode, counters: Arc<Counters>) {
    let mut connections = JoinSet::new();
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        counters.accepted.fetch_add(1, Ordering::SeqCst);
        match &mode {
            Mode::Drop => drop(stream),
            Mode::Mute => {
                connections.spawn(async move {
                    let _held = stream;
                    std::future::pending::<()>().await;
                });
            }
            Mode::Serve(handler) => {
                connections.spawn(connection(
                    stream,
                    Arc::clone(handler),
                    Arc::clone(&counters),
                ));
            }
        }
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
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
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

async fn until_closed(stream: &mut TcpStream, counters: &Counters) {
    let mut chunk = [0u8; 256];
    while matches!(stream.read(&mut chunk).await, Ok(n) if n > 0) {}
    counters.closed.fetch_add(1, Ordering::SeqCst);
}

async fn connection(mut stream: TcpStream, handler: Handler, counters: Arc<Counters>) {
    let Some(request) = read_request(&mut stream).await else {
        counters.closed.fetch_add(1, Ordering::SeqCst);
        return;
    };
    let action = handler(&request);
    counters.recorded.lock().expect("lock").push(request);
    counters.requests.fetch_add(1, Ordering::SeqCst);
    match action {
        Action::Reply(status, body) => {
            let _ = stream.write_all(&response(status, &body)).await;
        }
        Action::Raw(bytes) => {
            let _ = stream.write_all(&bytes).await;
        }
        Action::RawThenClose(bytes) => {
            let _ = stream.write_all(&bytes).await;
            counters.closed.fetch_add(1, Ordering::SeqCst);
            return;
        }
        Action::Close => {
            counters.closed.fetch_add(1, Ordering::SeqCst);
            return;
        }
        Action::Hang => {}
    }
    until_closed(&mut stream, &counters).await;
}
