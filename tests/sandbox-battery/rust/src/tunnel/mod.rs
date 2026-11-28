use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::env::Env;
use crate::error::{BatteryError, Res};
use crate::kubeconfig::TempContext;
use crate::proc::Kube;

mod serve;
mod state;
#[cfg(test)]
mod tests;

use state::State;

const SETTLE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateMode {
    Open,
    Refuse,
    Stall,
    DenyManifestWrite,
    HoldManifestWrite,
    PauseManifestWrite,
}

pub struct Gate {
    state: Arc<State>,
    port: u16,
    task: JoinHandle<()>,
}

impl Gate {
    pub async fn start(upstream: impl Into<String>, parse_http: bool) -> Res<Self> {
        let upstream = upstream.into();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let state = State::new();
        let shared = Arc::clone(&state);
        let task = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                shared.count_accepted();
                tokio::spawn(serve::serve(
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

    pub async fn set(&self, mode: GateMode) -> Res {
        self.state.apply(mode);
        if self.state.settled(SETTLE).await {
            return Ok(());
        }
        Err(BatteryError::NotReached {
            what: format!(
                "the gate to drop {} connection(s) of its previous setting",
                self.state.behind()
            ),
            seconds: SETTLE.as_secs(),
        })
    }

    pub fn accepted(&self) -> u64 {
        self.state.accepted()
    }

    pub fn live(&self) -> usize {
        self.state.live()
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        self.state.apply(GateMode::Refuse);
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
