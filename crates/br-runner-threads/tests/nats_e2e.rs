mod support;

use std::time::Duration;

use async_nats::jetstream::{self};
use br_agentic_contract::ThreadRef;
use br_runner_threads::transport::NatsBus;
use br_runner_threads::{ClosureKind, Error, MessageStatus, NewThread};
use contract_threads::{ChunkFrame, STREAMING_STREAM, chunk_subject};
use futures::future::join_all;
use support::nats::*;
use support::server::Server;
use support::*;

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn concurrent_commands_each_get_their_own_answer_even_when_answers_come_back_reversed() {
    let server = Server::start();
    let client = server.client().await;
    streams(&client, true).await;
    let script = Script::default();
    let threads: Vec<ThreadRef> = (0..12).map(|_| ThreadRef::new(v7())).collect();
    let requests: Vec<NewThread> = threads.iter().map(|t| new_thread_for(*t)).collect();
    for request in requests.iter().step_by(3) {
        script
            .refuse
            .lock()
            .expect("lock")
            .insert(request.command_id);
    }
    let _responder = responder(client.clone(), script.clone(), requests.len()).await;
    let threads_client = threads_client(&client).await;
    let results = join_all(requests.iter().map(|r| threads_client.create_thread(r))).await;
    for (index, (request, result)) in requests.iter().zip(results).enumerate() {
        if index % 3 == 0 {
            match result {
                Err(Error::Refused(refusal)) => {
                    assert_eq!(refusal.code, "KEY_REUSED");
                    assert_eq!(refusal.params["n"], 1);
                    assert_eq!(refusal.command_id, Some(request.command_id));
                }
                other => panic!("request {index} refused, not {other:?}"),
            }
        } else {
            assert_eq!(
                result.expect("confirmed").command_id,
                request.command_id,
                "request {index}"
            );
        }
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_missing_stream_fails_loud_and_nothing_is_created() {
    let server = Server::start();
    let client = server.client().await;
    streams(&client, false).await;
    match NatsBus::connect(client.clone()).await {
        Err(Error::Infra(text)) => assert!(text.contains(STREAMING_STREAM), "{text}"),
        other => panic!("infra, not {:?}", other.map(drop)),
    }
    let js = jetstream::new(client);
    assert!(js.get_stream(STREAMING_STREAM).await.is_err());
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn chunks_land_in_the_streaming_stream_in_order_with_their_dedup_id_and_the_closing_carries_the_status()
 {
    let server = Server::start();
    let client = server.client().await;
    streams(&client, true).await;
    let script = Script::default();
    let _responder = responder(client.clone(), script.clone(), 1).await;
    let threads_client = threads_client(&client).await;
    let thread = ThreadRef::new(v7());
    let request = open_request_for(thread);
    let handle = threads_client.open_stream(&request).await.expect("opened");
    handle.keepalive().await.expect("chunk");
    handle.entry(&notice()).await.expect("chunk");
    handle.keepalive().await.expect("chunk");

    let js = jetstream::new(client.clone());
    let stored = js.get_stream(STREAMING_STREAM).await.expect("stream");
    let message_id = handle.message_id();
    let mut seen = Vec::new();
    for sequence in 1..=3 {
        let raw = stored.get_raw_message(sequence).await.expect("raw");
        assert_eq!(raw.subject.as_str(), chunk_subject(message_id));
        let id = raw
            .headers
            .get(async_nats::header::NATS_MESSAGE_ID)
            .expect("dedup id")
            .as_str()
            .to_owned();
        let frame: ChunkFrame = serde_json::from_slice(&raw.payload).expect("frame");
        assert_eq!(id, format!("{message_id}:{}", frame.seq));
        seen.push(frame.seq);
    }
    assert_eq!(seen, [0, 1, 2]);

    *script.status.lock().expect("lock") = Some("COMPLETED");
    let closing = closing(&handle, thread, ClosureKind::Completed);
    let confirmed = threads_client
        .close_message(&closing, Some(&handle))
        .await
        .expect("closed");
    assert_eq!(confirmed.status, Some(MessageStatus::Completed));
    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn an_answer_with_an_unknown_status_is_unreadable() {
    let server = Server::start();
    let client = server.client().await;
    streams(&client, true).await;
    let script = Script::default();
    let _responder = responder(client.clone(), script.clone(), 1).await;
    let threads_client = threads_client(&client).await;
    let thread = ThreadRef::new(v7());
    *script.garbage.lock().expect("lock") = true;
    let result = threads_client.open_stream(&open_request_for(thread)).await;
    assert!(
        matches!(result, Err(Error::Unreadable(_))),
        "{:?}",
        result.map(drop)
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn no_answer_within_the_importer_wait_is_an_answer_lost() {
    let server = Server::start();
    let client = server.client().await;
    streams(&client, true).await;
    let threads_client = threads_client(&client).await;
    let mut request = new_thread_for(ThreadRef::new(v7()));
    request.wait = Some(Duration::from_millis(200));
    let result = threads_client.create_thread(&request).await;
    assert!(
        matches!(result, Err(Error::AnswerLost(_))),
        "{:?}",
        result.map(drop)
    );
}

fn open_request_for(thread: ThreadRef) -> br_runner_threads::OpenStream {
    let mut request = br_runner_threads::OpenStream {
        command_id: v7(),
        thread,
        message_id: turn_id(),
        parent_message_id: None,
        agent_id: agent_id(),
        author_name: "Analyst".to_owned(),
        job_id: None,
        acting_user_id: None,
        wait: Some(Duration::from_secs(10)),
    };
    request.parent_message_id = Some(v7());
    request
}
