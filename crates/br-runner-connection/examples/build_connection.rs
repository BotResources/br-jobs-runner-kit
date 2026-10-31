use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, Error, FixedToken};

pub fn build_connection(
    endpoint: &str,
    token: &str,
    bus: async_nats::Client,
    service_account: ServiceAccountId,
) -> Result<Connection, Error> {
    Connection::new(
        endpoint,
        Arc::new(FixedToken::new(token)),
        bus,
        service_account,
        Duration::from_secs(30),
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, endpoint, nats_url, token, account] = args.as_slice() else {
        return Err(
            "usage: build_connection <endpoint> <nats-url> <token> <service-account-uuid>".into(),
        );
    };
    let bus = async_nats::connect(nats_url).await?;
    let service_account = ServiceAccountId(account.parse()?);
    let connection = build_connection(endpoint, token, bus, service_account)?;
    let data = connection
        .graphql("query { me { id } }", serde_json::json!({}))
        .await?;
    println!("{data}");
    Ok(())
}
