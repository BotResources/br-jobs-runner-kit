use br_runner_connection::{Connection, Error};
use serde_json::{Value, json};

pub async fn me(connection: &Connection) -> Result<Value, Error> {
    connection.graphql("query { me { id } }", json!({})).await
}

pub fn describe(result: Result<Value, Error>) -> String {
    match result {
        Ok(data) => format!("data {data}"),
        Err(Error::InvalidInput(reason)) => format!("nothing sent, fix the input: {reason}"),
        Err(Error::Credential(failure)) => format!("nothing sent, no token: {failure}"),
        Err(Error::Unavailable(failure)) => {
            format!("nothing received, send again if you want: {failure}")
        }
        Err(Error::Lost(failure)) => {
            format!("sent, no usable answer, it may have been applied: {failure}")
        }
        Err(Error::Unauthenticated) => "the gateway rejected the token".to_owned(),
        Err(Error::Refused(refusal)) => match refusal.code.as_str() {
            "NOT_FOUND" => "the service says it found nothing".to_owned(),
            code => format!("refused with the code {code}: {}", refusal.message),
        },
        Err(Error::Unreadable(reason)) => format!("an answer came that cannot be read: {reason}"),
        Err(other) => format!("an error kind of a later version: {other}"),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, endpoint, nats_url, token, account] = args.as_slice() else {
        return Err(
            "usage: graphql_call <endpoint> <nats-url> <token> <service-account-uuid>".into(),
        );
    };
    let connection = Connection::new(
        endpoint,
        std::sync::Arc::new(br_runner_connection::FixedToken::new(token.as_str())),
        async_nats::connect(nats_url).await?,
        br_core_kernel::ServiceAccountId(account.parse()?),
        std::time::Duration::from_secs(30),
    )?;
    println!("{}", describe(me(&connection).await));
    Ok(())
}
