use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_llm_messages::Turn;
use br_runner_connection::{Connection, FixedToken};
use br_runner_threads::{
    Closing, ClosureKind, Error, NewThread, OpenStream, ThreadClient, WholeMessage,
};
use uuid::Uuid;

use super::ids::{agent_id, human_entry, input_turn, turn_id, v7};

pub const WAIT: Duration = Duration::from_secs(10);
pub const CHECK_WAIT: Duration = Duration::from_secs(5);
pub const OFFLINE_ENDPOINT: &str = "http://127.0.0.1:9/graphql";
pub const BEARER: &str = "tok-for-the-stand-in";

pub fn account() -> ServiceAccountId {
    ServiceAccountId(Uuid::from_u128(0xA0))
}

pub fn connection(bus: &async_nats::Client, endpoint: &str) -> Connection {
    connection_with_reply_wait(bus, endpoint, Duration::from_secs(5))
}

pub fn connection_with_reply_wait(
    bus: &async_nats::Client,
    endpoint: &str,
    reply_wait: Duration,
) -> Connection {
    Connection::new(
        endpoint,
        Arc::new(FixedToken::new(BEARER)),
        bus.clone(),
        account(),
        reply_wait,
    )
    .expect("a connection")
}

pub async fn built(bus: &async_nats::Client) -> ThreadClient {
    ThreadClient::new(&connection(bus, OFFLINE_ENDPOINT), CHECK_WAIT)
        .await
        .expect("the client is built")
}

pub async fn build_error(bus: &async_nats::Client, check_wait: Duration) -> Error {
    match ThreadClient::new(&connection(bus, OFFLINE_ENDPOINT), check_wait).await {
        Err(error) => error,
        Ok(_) => panic!("the client was built"),
    }
}

pub fn new_thread() -> NewThread {
    NewThread {
        command_id: v7(),
        thread_id: v7(),
        owner_id: Some(v7()),
        member_ids: vec![v7()],
        title: Some("A title".to_owned()),
        resources: BTreeMap::from([("kms".to_owned(), vec!["km-1".to_owned()])]),
        origin: None,
        acting_user_id: None,
        wait: WAIT,
    }
}

pub fn whole_message(thread_id: Uuid) -> WholeMessage {
    WholeMessage {
        command_id: v7(),
        thread_id,
        parent_message_id: None,
        agent_id: agent_id(),
        author_name: "Analyst".to_owned(),
        job_id: None,
        acting_user_id: None,
        impersonator_id: None,
        turn: Turn::InputTurn(input_turn(vec![human_entry("hello")])),
        annotations: Vec::new(),
        wait: WAIT,
    }
}

pub fn open_stream(thread_id: Uuid) -> OpenStream {
    OpenStream {
        command_id: v7(),
        thread_id,
        message_id: turn_id(),
        parent_message_id: None,
        agent_id: agent_id(),
        author_name: "Analyst".to_owned(),
        job_id: None,
        acting_user_id: None,
        wait: WAIT,
    }
}

pub fn closing(open: &OpenStream, kind: ClosureKind) -> Closing {
    Closing {
        command_id: v7(),
        thread_id: open.thread_id,
        message_id: open.message_id.as_uuid(),
        kind,
        detail: None,
        acting_user_id: None,
        wait: WAIT,
    }
}

pub async fn until<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..400 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("never true: {what}");
}
