// each test file includes this module and uses only part of it
#![allow(dead_code, unused_imports)]

pub mod nats;
pub mod server;
pub mod tls;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::{BoxFuture, Connection, Credential, CredentialFailure, Error, Token};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

pub use server::{Action, HttpServer, Mode, serve};
pub use tls::UntrustedTlsServer;

pub const TOKEN: &str = "tok-0123456789-SECRET-abcdef";

pub fn account() -> ServiceAccountId {
    ServiceAccountId(Uuid::from_u128(0xA0))
}

pub async fn idle_bus() -> async_nats::Client {
    async_nats::ConnectOptions::new()
        .retry_on_initial_connect()
        .connect("nats://127.0.0.1:1")
        .await
        .expect("a client that retries")
}

pub type Behaviour = Box<dyn Fn() -> Action + Send + Sync>;

pub enum Gate {
    Open,
    Closed(tokio::sync::oneshot::Receiver<()>),
}

pub struct Counting {
    pub fetched: Arc<AtomicUsize>,
    failure: Option<CredentialFailure>,
    gate: Mutex<Gate>,
}

impl Counting {
    pub fn giving_token() -> Arc<Self> {
        Arc::new(Self {
            fetched: Arc::default(),
            failure: None,
            gate: Mutex::new(Gate::Open),
        })
    }

    pub fn failing(failure: CredentialFailure) -> Arc<Self> {
        Arc::new(Self {
            fetched: Arc::default(),
            failure: Some(failure),
            gate: Mutex::new(Gate::Open),
        })
    }

    pub fn gated() -> (Arc<Self>, tokio::sync::oneshot::Sender<()>) {
        let (release, wait) = tokio::sync::oneshot::channel();
        let credential = Arc::new(Self {
            fetched: Arc::default(),
            failure: None,
            gate: Mutex::new(Gate::Closed(wait)),
        });
        (credential, release)
    }

    pub fn fetches(&self) -> usize {
        self.fetched.load(Ordering::SeqCst)
    }
}

impl Credential for Counting {
    fn token(&self) -> BoxFuture<'_, Result<Token, CredentialFailure>> {
        Box::pin(async move {
            self.fetched.fetch_add(1, Ordering::SeqCst);
            let mut gate = self.gate.lock().await;
            if let Gate::Closed(wait) = &mut *gate {
                let _ = wait.await;
                *gate = Gate::Open;
            }
            match &self.failure {
                Some(failure) => Err(failure.clone()),
                None => Ok(Token::new(TOKEN)),
            }
        })
    }
}

pub async fn connection_to(
    url: &str,
    credential: Arc<dyn Credential>,
    wait: Duration,
) -> Connection {
    Connection::new(url, credential, idle_bus().await, account(), wait).expect("builds")
}

pub fn data(value: Value) -> Action {
    Action::json(200, json!({ "data": value }))
}

pub async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

pub async fn answered(action: impl Fn() -> Action + Send + Sync + 'static) -> Result<Value, Error> {
    let server = HttpServer::start(serve(move |_| action())).await;
    let connection = connection_to(
        &server.url(),
        Counting::giving_token(),
        Duration::from_secs(10),
    )
    .await;
    connection.graphql("{ x }", json!({})).await
}

pub fn refused(result: Result<Value, Error>) -> br_runner_connection::Refusal {
    match result {
        Err(Error::Refused(refusal)) => refusal,
        other => panic!("expected Refused, got {other:?}"),
    }
}

pub async fn bounded<T>(work: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(20), work)
        .await
        .expect("the call returned in time: its deadline code is broken")
}
