use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::env::Env;
use crate::error::Res;
use crate::kubeconfig::TempContext;
use crate::proc::Kube;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateMode {
    Open,
    Refuse,
    Stall,
    DenyManifestWrite,
    HoldManifestWrite,
    PauseManifestWrite,
}

impl GateMode {
    fn code(self) -> u8 {
        match self {
            Self::Open => 0,
            Self::Refuse => 1,
            Self::Stall => 2,
            Self::DenyManifestWrite => 3,
            Self::HoldManifestWrite => 4,
            Self::PauseManifestWrite => 5,
        }
    }

    fn of(code: u8) -> Self {
        match code {
            1 => Self::Refuse,
            2 => Self::Stall,
            3 => Self::DenyManifestWrite,
            4 => Self::HoldManifestWrite,
            5 => Self::PauseManifestWrite,
            _ => Self::Open,
        }
    }
}

struct State {
    mode: AtomicU8,
    epoch: AtomicU64,
    accepted: AtomicU64,
}

impl State {
    fn mode(&self) -> GateMode {
        GateMode::of(self.mode.load(Ordering::SeqCst))
    }
}

pub struct Gate {
    state: Arc<State>,
    port: u16,
    task: JoinHandle<()>,
}

const DENIED: &[u8] = b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

async fn read_head(client: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 16_384 || client.read(&mut byte).await.ok()? == 0 {
            return None;
        }
        head.push(byte[0]);
    }
    Some(head)
}

fn closing(head: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(head);
    let mut out = String::new();
    for line in text.split("\r\n").filter(|line| !line.is_empty()) {
        if !line.to_ascii_lowercase().starts_with("connection:") {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    out.push_str("Connection: close\r\n\r\n");
    out.into_bytes()
}

async fn serve(state: Arc<State>, mut client: TcpStream, upstream: String, parse: bool) {
    let epoch = state.epoch.load(Ordering::SeqCst);
    let mode = state.mode();
    match mode {
        GateMode::Refuse => {}
        GateMode::Stall => {
            while state.epoch.load(Ordering::SeqCst) == epoch {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        GateMode::Open
        | GateMode::DenyManifestWrite
        | GateMode::HoldManifestWrite
        | GateMode::PauseManifestWrite => {
            let Ok(mut server) = TcpStream::connect(&upstream).await else {
                return;
            };
            let mut epoch = epoch;
            if parse
                && matches!(
                    mode,
                    GateMode::DenyManifestWrite
                        | GateMode::HoldManifestWrite
                        | GateMode::PauseManifestWrite
                )
            {
                let Some(head) = read_head(&mut client).await else {
                    return;
                };
                let first = String::from_utf8_lossy(&head);
                let line = first.lines().next().unwrap_or("");
                if line.starts_with("PUT ") && line.contains("manifest.json") {
                    match mode {
                        GateMode::HoldManifestWrite => {
                            while state.epoch.load(Ordering::SeqCst) == epoch {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                            }
                            return;
                        }
                        GateMode::PauseManifestWrite => {
                            while state.epoch.load(Ordering::SeqCst) == epoch {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                            }
                            epoch = state.epoch.load(Ordering::SeqCst);
                        }
                        _ => {
                            let _ = client.write_all(DENIED).await;
                            return;
                        }
                    }
                }
                if server.write_all(&closing(&head)).await.is_err() {
                    return;
                }
            }
            let watch = async {
                while state.epoch.load(Ordering::SeqCst) == epoch {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            };
            tokio::select! {
                _ = copy_bidirectional(&mut client, &mut server) => {}
                () = watch => {}
            }
        }
    }
}

impl Gate {
    pub async fn start(upstream: impl Into<String>, parse_http: bool) -> Res<Self> {
        let upstream = upstream.into();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let state = Arc::new(State {
            mode: AtomicU8::new(GateMode::Open.code()),
            epoch: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
        });
        let shared = Arc::clone(&state);
        let task = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                shared.accepted.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serve(
                    Arc::clone(&shared),
                    client,
                    upstream.clone(),
                    parse_http,
                ));
            }
        });
        Ok(Self { state, port, task })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn set(&self, mode: GateMode) {
        self.state.mode.store(mode.code(), Ordering::SeqCst);
        self.state.epoch.fetch_add(1, Ordering::SeqCst);
    }

    pub fn accepted(&self) -> u64 {
        self.state.accepted.load(Ordering::SeqCst)
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        self.set(GateMode::Refuse);
        self.task.abort();
    }
}

pub async fn api_gate(env: &Env, name: &str) -> Res<(Gate, TempContext)> {
    let raw = Kube::cluster_wide(env)
        .run(&[
            "config",
            "view",
            "--raw",
            "--minify",
            "--context",
            &env.context,
            "-o",
            "jsonpath={.clusters[0].cluster.server}",
        ])
        .await?;
    let upstream = raw
        .trim_start_matches("https://")
        .replace("0.0.0.0", "127.0.0.1");
    let gate = Gate::start(upstream, false).await?;
    let context = TempContext::through_gate(env, name, gate.port()).await?;
    Ok((gate, context))
}
