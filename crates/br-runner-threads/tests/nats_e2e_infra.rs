mod support;

use std::time::Duration;

use async_nats::jetstream;
use br_agentic_contract::ThreadRef;
use br_runner_threads::Error;
use support::nats::*;
use support::server::Server;
use support::*;

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_resend_under_the_same_command_id_reaches_threads_each_time() {
    let server = Server::start();
    let client = server.client().await;
    streams(&client, true).await;
    let threads_client = threads_client(&client).await;
    let mut request = new_thread_for(ThreadRef::new(v7()));
    request.wait = Some(Duration::from_millis(200));
    assert!(matches!(
        threads_client.create_thread(&request).await,
        Err(Error::AnswerLost(_))
    ));

    let _responder = responder(client.clone(), Script::default(), 1).await;
    request.wait = Some(Duration::from_secs(10));
    for _ in 0..2 {
        let confirmed = threads_client
            .create_thread(&request)
            .await
            .expect("a resend is answered");
        assert_eq!(confirmed.command_id, request.command_id);
    }

    let stored = stored_commands(&client).await;
    assert_eq!(stored.len(), 3);
    assert!(
        stored
            .iter()
            .all(|command| command["command_id"] == serde_json::json!(request.command_id))
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_command_published_after_its_stream_was_deleted_is_unavailable() {
    let server = Server::start();
    let client = server.client().await;
    streams(&client, true).await;
    let threads_client = threads_client(&client).await;
    jetstream::new(client.clone())
        .delete_stream("INTEGRATION_CMD")
        .await
        .expect("deleted");
    let result = threads_client
        .create_thread(&new_thread_for(ThreadRef::new(v7())))
        .await;
    assert!(
        matches!(result, Err(Error::Unavailable(_))),
        "{:?}",
        result.map(drop)
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_connection_that_is_not_connected_is_unavailable() {
    let server = Server::start();
    let client = server.client().await;
    streams(&client, true).await;
    let threads_client = threads_client(&client).await;
    drop(server);
    let mut request = new_thread_for(ThreadRef::new(v7()));
    request.wait = Some(Duration::from_millis(200));
    for _ in 0..100 {
        if matches!(
            threads_client.create_thread(&request).await,
            Err(Error::Unavailable(_))
        ) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the lost connection never gave unavailable");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn waiters_lose_their_answer_when_the_answer_subscription_closes() {
    let server = Server::start();
    let client = server.client().await;
    streams(&client, true).await;
    let threads_client = threads_client(&client).await;
    let request = new_thread_for(ThreadRef::new(v7()));
    let waiting = {
        let threads_client = threads_client.clone();
        tokio::spawn(async move { threads_client.create_thread(&request).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    client.drain().await.expect("drained");
    let result = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .expect("the waiter ends")
        .expect("task");
    assert!(
        matches!(result, Err(Error::AnswerLost(_))),
        "{:?}",
        result.map(drop)
    );
}
