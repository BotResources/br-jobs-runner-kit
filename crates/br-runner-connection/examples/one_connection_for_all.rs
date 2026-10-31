use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, Error, FixedToken};
use bytes::Bytes;
use serde_json::{Value, json};

pub async fn threads_side(connection: Connection) -> Result<Value, Error> {
    connection.graphql("query { me { id } }", json!({})).await
}

pub async fn jobs_side(connection: Connection) -> Result<Bytes, Error> {
    connection
        .bus_request(
            "example.echo",
            Bytes::from_static(b"ping"),
            Duration::from_secs(5),
        )
        .await
}

pub async fn run(connection: &Connection) -> Result<(Value, Bytes), Error> {
    let (threads, jobs) = tokio::join!(
        threads_side(connection.clone()),
        jobs_side(connection.clone())
    );
    Ok((threads?, jobs?))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, endpoint, nats_url, token, account] = args.as_slice() else {
        return Err(
            "usage: one_connection_for_all <endpoint> <nats-url> <token> <service-account-uuid>"
                .into(),
        );
    };
    let connection = Connection::new(
        endpoint,
        Arc::new(FixedToken::new(token.as_str())),
        async_nats::connect(nats_url).await?,
        ServiceAccountId(account.parse()?),
        Duration::from_secs(30),
    )?;
    let (threads, jobs) = run(&connection).await?;
    println!("{threads} {}", String::from_utf8_lossy(&jobs));
    Ok(())
}
