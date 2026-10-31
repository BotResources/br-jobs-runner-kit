mod support;

#[path = "../examples/runner_wiring.rs"]
#[allow(dead_code)] // the example is included for its functions; its main is unused here
mod runner_wiring;

use br_runner_threads::test_double;
use std::time::Duration;

use br_agentic_contract::{ThreadAccess, ThreadRef};
use br_llm_messages::{AgentId, Author, EntryId};
use br_runner_threads::test_double::{CommandBody, MemoryThreads};
use br_runner_threads::{Error, ThreadAccessAdapter, ThreadClient};
use contract_threads::ReasonCode;
use runner_wiring::{
    Mark, build_connection, build_thread_access, lifecycle_hook, mail_turn, node_entry,
};
use support::adapter_wire::stores;
use support::nats_fixtures::{BEARER, CHECK_WAIT, OFFLINE_ENDPOINT, account, new_thread};
use support::nats_responder::{Reply, Responder};
use support::nats_server::Server;
use support::nats_streams::create_all_streams;
use support::queue_kit::within;
use support::*;

struct Wiring {
    threads: MemoryThreads,
    adapter: ThreadAccessAdapter,
    agent: AgentId,
    thread: ThreadRef,
}

async fn wiring() -> Wiring {
    let threads = MemoryThreads::new();
    let agent = agent_id();
    let adapter = build_thread_access(
        &threads.client(),
        agent,
        Author::new("Analyst").expect("author"),
    )
    .await
    .expect("the thread access");
    Wiring {
        threads,
        adapter,
        agent,
        thread: thread_ref(),
    }
}

fn mail() -> (EntryId, br_llm_messages::InputTurn) {
    mail_turn("Please summarize the open tickets").expect("a mail turn")
}

#[tokio::test]
async fn a_mail_is_marked_read_when_threads_has_confirmed_its_input_turn() {
    let wiring = wiring().await;
    let (mail, turn) = mail();
    let turn_id = turn.id();

    let mark = within(
        "the mark",
        node_entry(&wiring.adapter, &wiring.thread, mail, turn),
    )
    .await
    .expect("accepted");

    assert_eq!(mark, Mark::Read);
    let stored = stores(&wiring.threads);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].message_id, turn_id.as_uuid());
    assert_eq!(stored[0].thread_id, wiring.thread.as_uuid());
}

#[tokio::test]
async fn the_mark_waits_while_threads_does_not_take_the_input_turn() {
    let wiring = wiring().await;
    wiring.threads.pause();
    let (mail, turn) = mail();
    let marking = node_entry(&wiring.adapter, &wiring.thread, mail, turn);
    tokio::pin!(marking);

    let early = tokio::time::timeout(Duration::from_millis(200), &mut marking).await;

    assert!(early.is_err());
    wiring.threads.resume();
    let mark = within("the mark", marking).await.expect("accepted");
    assert_eq!(mark, Mark::Read);
}

#[tokio::test]
async fn a_mail_is_left_unread_when_threads_refuses_its_input_turn_for_good() {
    let wiring = wiring().await;
    wiring.threads.answer_commands(|command| {
        Err(Error::Refused(test_double::refusal(
            "THREAD_NOT_OPEN",
            "the thread is closed",
            Some(command.id),
        )))
    });
    let (mail, turn) = mail();

    let mark = within(
        "the mark",
        node_entry(&wiring.adapter, &wiring.thread, mail, turn),
    )
    .await
    .expect("accepted");

    assert_eq!(mark, Mark::LeaveUnread);
}

#[tokio::test]
async fn the_hook_returns_when_the_thread_queue_of_the_ended_agent_is_delivered() {
    let wiring = wiring().await;
    wiring.threads.pause();
    let (_, turn) = mail();
    wiring
        .adapter
        .add_input_turn(&wiring.thread, turn)
        .await
        .expect("accepted");
    let ended = [wiring.agent];
    let hook = lifecycle_hook(&wiring.adapter, &ended);
    tokio::pin!(hook);

    let early = tokio::time::timeout(Duration::from_millis(200), &mut hook).await;

    assert!(early.is_err());
    wiring.threads.resume();
    within("the hook", hook).await;
    assert!(
        wiring
            .threads
            .commands()
            .iter()
            .any(|command| matches!(command.body, CommandBody::Store(_)))
    );
}

#[tokio::test]
async fn the_hook_returns_at_once_for_an_agent_the_adapter_knows_nothing_of() {
    let wiring = wiring().await;

    within(
        "the hook",
        lifecycle_hook(&wiring.adapter, &[agent_id(), agent_id()]),
    )
    .await;

    assert!(wiring.threads.operations().is_empty());
}

#[tokio::test]
async fn a_mail_turn_holds_one_entry_that_carries_the_mail_id() {
    let (mail, turn) = mail();

    assert_eq!(turn.entries().len(), 1);
    assert_eq!(turn.entries()[0].id(), mail);
}

struct RealBus {
    _server: Server,
    responder: Responder,
    client: ThreadClient,
    adapter: ThreadAccessAdapter,
    agent: AgentId,
}

async fn real_bus() -> RealBus {
    let server = Server::start();
    let bus = server.client().await;
    create_all_streams(&bus).await;
    let responder = Responder::start(&bus).await;
    let connection =
        build_connection(OFFLINE_ENDPOINT, BEARER, bus, account()).expect("a connection");
    let client = ThreadClient::new(&connection, CHECK_WAIT)
        .await
        .expect("the client");
    let agent = agent_id();
    let author = Author::new("Analyst").expect("author");
    let adapter = build_thread_access(&client, agent, author)
        .await
        .expect("the thread access");
    RealBus {
        _server: server,
        responder,
        client,
        adapter,
        agent,
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn the_quickstart_creates_a_thread_stores_a_mail_and_marks_it_read_over_a_real_bus() {
    let bus = real_bus().await;
    let creation = new_thread();
    let thread = ThreadRef::from_uuid(creation.thread_id).expect("thread ref");
    bus.client.create_thread(&creation).await.expect("created");
    let (mail, turn) = mail();

    let mark = within("the mark", node_entry(&bus.adapter, &thread, mail, turn))
        .await
        .expect("accepted");
    within("the hook", lifecycle_hook(&bus.adapter, &[bus.agent])).await;

    assert_eq!(mark, Mark::Read);
    let kinds: Vec<String> = bus
        .responder
        .received()
        .iter()
        .map(|arrival| arrival.command_type().to_owned())
        .collect();
    assert_eq!(kinds.len(), 2);
    assert_eq!(kinds[1], "message.store");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn the_quickstart_leaves_the_mail_unread_when_threads_refuses_the_input_turn_over_a_real_bus()
{
    let bus = real_bus().await;
    bus.responder
        .script_arrivals([Reply::Refuse(ReasonCode::ThreadNotOpen, Default::default())]);
    let (mail, turn) = mail();

    let mark = within(
        "the mark",
        node_entry(&bus.adapter, &thread_ref(), mail, turn),
    )
    .await
    .expect("accepted");

    assert_eq!(mark, Mark::LeaveUnread);
}
