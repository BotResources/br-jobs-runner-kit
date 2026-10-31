mod support;

#[path = "../examples/build_connection.rs"]
#[allow(dead_code)]
mod build_connection;
#[path = "../examples/custom_credential.rs"]
#[allow(dead_code)]
mod custom_credential;
#[path = "../examples/graphql_call.rs"]
#[allow(dead_code)]
mod graphql_call;
#[path = "../examples/one_connection_for_all.rs"]
#[allow(dead_code)]
mod one_connection_for_all;

use br_runner_connection::Connection;
use bytes::Bytes;
use serde_json::json;
use support::nats::{Server, WAIT, echo};
use support::*;

fn graphql_server() -> Mode {
    serve(|request| {
        let authorized = request.header("authorization") == Some(&format!("Bearer {TOKEN}"));
        match authorized {
            true => data(json!({ "me": { "id": "u1" } })),
            false => Action::json(401, json!({})),
        }
    })
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn build_connection_builds_a_connection_that_makes_a_call_against_a_real_endpoint_and_bus() {
    let server = HttpServer::start(graphql_server()).await;
    let nats = Server::start();
    let connection =
        build_connection::build_connection(&server.url(), TOKEN, nats.client().await, account())
            .expect("builds");
    let data = connection
        .graphql("query { me { id } }", json!({}))
        .await
        .expect("data");
    assert_eq!(data, json!({ "me": { "id": "u1" } }));
    assert_eq!(connection.service_account(), account());
}

#[tokio::test]
async fn graphql_call_reads_data_and_each_error_kind() {
    let server = HttpServer::start(graphql_server()).await;
    let connection = connection_to(&server.url(), Counting::giving_token(), WAIT).await;
    let described = graphql_call::describe(graphql_call::me(&connection).await);
    assert_eq!(described, r#"data {"me":{"id":"u1"}}"#);

    let refusing = HttpServer::start(serve(|_| {
        Action::json(
            200,
            json!({ "errors": [{ "message": "gone", "extensions": { "code": "NOT_FOUND" } }] }),
        )
    }))
    .await;
    let connection = connection_to(&refusing.url(), Counting::giving_token(), WAIT).await;
    let described = graphql_call::describe(graphql_call::me(&connection).await);
    assert_eq!(described, "the service says it found nothing");

    let other = HttpServer::start(serve(|_| {
        Action::json(
            200,
            json!({ "errors": [{ "message": "no", "extensions": { "code": "QUOTA" } }] }),
        )
    }))
    .await;
    let connection = connection_to(&other.url(), Counting::giving_token(), WAIT).await;
    let described = graphql_call::describe(graphql_call::me(&connection).await);
    assert_eq!(described, "refused with the code QUOTA: no");

    let unauthenticated = HttpServer::start(serve(|_| Action::json(401, json!({})))).await;
    let connection = connection_to(&unauthenticated.url(), Counting::giving_token(), WAIT).await;
    let described = graphql_call::describe(graphql_call::me(&connection).await);
    assert_eq!(described, "the gateway rejected the token");

    let garbled = HttpServer::start(serve(|_| Action::Reply(200, b"not json".to_vec()))).await;
    let connection = connection_to(&garbled.url(), Counting::giving_token(), WAIT).await;
    let described = graphql_call::describe(graphql_call::me(&connection).await);
    assert!(
        described.starts_with("an answer came that cannot be read"),
        "{described}"
    );

    let failing = HttpServer::start(serve(|_| Action::Reply(500, Vec::new()))).await;
    let connection = connection_to(&failing.url(), Counting::giving_token(), WAIT).await;
    let described = graphql_call::describe(graphql_call::me(&connection).await);
    assert!(
        described.starts_with("sent, no usable answer"),
        "{described}"
    );

    let closed = HttpServer::start(Mode::Drop).await;
    let url = closed.url();
    drop(closed);
    let connection = connection_to(&url, Counting::giving_token(), WAIT).await;
    let described = graphql_call::describe(graphql_call::me(&connection).await);
    assert!(described.starts_with("nothing received"), "{described}");

    let failing_credential =
        Counting::failing(br_runner_connection::CredentialFailure::no_usable("none"));
    let connection = connection_to(&refusing.url(), failing_credential, WAIT).await;
    let described = graphql_call::describe(graphql_call::me(&connection).await);
    assert!(
        described.starts_with("nothing sent, no token"),
        "{described}"
    );

    let described = graphql_call::describe(connection.graphql("", json!({})).await);
    assert!(
        described.starts_with("nothing sent, fix the input"),
        "{described}"
    );
}

#[tokio::test]
async fn custom_credential_fetches_a_token_for_a_call_and_fails_with_each_of_its_two_kinds() {
    let server = HttpServer::start(graphql_server()).await;
    let outcomes = custom_credential::run(&server.url(), TOKEN, idle_bus().await, account())
        .await
        .expect("the three outcomes");
    assert_eq!(
        outcomes,
        [
            r#"data {"me":{"id":"u1"}}"#.to_owned(),
            "fetch failed: the vault is down".to_owned(),
            "no usable credential: the vault holds no token".to_owned(),
        ]
    );
    assert_eq!(
        server.requests().len(),
        1,
        "a failing credential sends nothing"
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn one_connection_for_all_serves_a_graphql_call_and_a_bus_request_at_the_same_time() {
    let server = HttpServer::start(graphql_server()).await;
    let nats = Server::start();
    echo(nats.client().await, "example.echo").await;
    let connection = Connection::new(
        &server.url(),
        Counting::giving_token(),
        nats.client().await,
        account(),
        WAIT,
    )
    .expect("builds");
    let (threads, jobs) = one_connection_for_all::run(&connection)
        .await
        .expect("both sides");
    assert_eq!(threads, json!({ "me": { "id": "u1" } }));
    assert_eq!(jobs, Bytes::from_static(b"echo:ping"));
}
