use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::{
    BoxFuture, Connection, Credential, CredentialFailure, CredentialKind, Error, Token,
};
use serde_json::{Value, json};

pub enum Vault {
    Holds(String),
    Down,
    Empty,
}

impl Credential for Vault {
    fn token(&self) -> BoxFuture<'_, Result<Token, CredentialFailure>> {
        Box::pin(async move {
            match self {
                Vault::Holds(secret) => Ok(Token::new(secret.clone())),
                Vault::Down => Err(CredentialFailure::fetch_failed("the vault is down")),
                Vault::Empty => Err(CredentialFailure::no_usable("the vault holds no token")),
            }
        })
    }
}

async fn me(
    endpoint: &str,
    vault: Vault,
    bus: async_nats::Client,
    service_account: ServiceAccountId,
) -> Result<Value, Error> {
    Connection::new(
        endpoint,
        Arc::new(vault),
        bus,
        service_account,
        Duration::from_secs(30),
    )?
    .graphql("query { me { id } }", json!({}))
    .await
}

pub async fn run(
    endpoint: &str,
    secret: &str,
    bus: async_nats::Client,
    service_account: ServiceAccountId,
) -> Result<Vec<String>, Error> {
    let mut outcomes = Vec::new();
    let data = me(
        endpoint,
        Vault::Holds(secret.to_owned()),
        bus.clone(),
        service_account,
    )
    .await?;
    outcomes.push(format!("data {data}"));
    for vault in [Vault::Down, Vault::Empty] {
        match me(endpoint, vault, bus.clone(), service_account).await {
            Err(Error::Credential(failure)) => outcomes.push(match failure.kind {
                CredentialKind::FetchFailed => format!("fetch failed: {}", failure.text),
                CredentialKind::NoUsable => format!("no usable credential: {}", failure.text),
                _ => format!("a kind of a later version: {}", failure.text),
            }),
            other => {
                return Err(Error::InvalidInput(format!(
                    "expected a Credential error, got {other:?}"
                )));
            }
        }
    }
    Ok(outcomes)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, endpoint, nats_url, token, account] = args.as_slice() else {
        return Err(
            "usage: custom_credential <endpoint> <nats-url> <token> <service-account-uuid>".into(),
        );
    };
    let bus = async_nats::connect(nats_url).await?;
    for outcome in run(endpoint, token, bus, ServiceAccountId(account.parse()?)).await? {
        println!("{outcome}");
    }
    Ok(())
}
