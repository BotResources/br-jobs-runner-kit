use std::sync::Arc;
use std::time::Duration;

use br_runner_sandbox::Clock;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::environment::{ACCESS_KEY_VARIABLE, SECRET_KEY_VARIABLE, StoreCredential};
use crate::facts::{Phase, PodFacts, fixtures};
use crate::fakes::{FakeKube, TokioClock};
use crate::http::tests::request;
use crate::runner::{Parts, run_on};
use crate::time::at;

const ACCESS: &str = "AKIAKNOWNACCESSID7Q";
const SECRET: &str = "known-secret-value-3f9c1b7e";
const START: u64 = 1_000_000;

fn credential() -> StoreCredential {
    let environment = [(ACCESS_KEY_VARIABLE, ACCESS), (SECRET_KEY_VARIABLE, SECRET)];
    StoreCredential::from_lookup(|name| {
        environment
            .iter()
            .find(|(variable, _)| *variable == name)
            .map(|(_, value)| (*value).to_owned())
    })
    .unwrap()
}

fn bucket_of(stub: &Stub) -> S3Bucket {
    let credential = credential();
    let mut settings = testing::config().store;
    settings.endpoint = Url::parse(&format!("http://{}", stub.address)).unwrap();
    settings.path_style = true;
    S3Bucket::new(
        &settings,
        credential.access_key_id(),
        credential.secret_access_key(),
    )
    .unwrap()
}

fn refusals(count: usize) -> Vec<Canned> {
    (0..count)
        .map(|_| canned(403, error_body("AccessDenied")))
        .collect()
}

fn holds_no_secret(text: &str) -> bool {
    !text.contains(SECRET) && !text.contains(ACCESS)
}

#[tokio::test]
async fn the_errors_of_every_store_call_that_fails_hold_no_credential() {
    let stub = Stub::serving(refusals(8)).await;
    let bucket = bucket_of(&stub);
    let errors = [
        bucket.check().await.unwrap_err(),
        bucket.list_page("saves/", None, 10).await.unwrap_err(),
        bucket.get("saves/x").await.unwrap_err(),
        bucket
            .put("tombstones/x", Bytes::from_static(b"{}"))
            .await
            .unwrap_err(),
        bucket.delete("saves/x").await.unwrap_err(),
    ];
    for error in &errors {
        assert!(holds_no_secret(&error.to_string()), "{error}");
        assert!(holds_no_secret(&format!("{error:?}")), "{error:?}");
    }
}

#[tokio::test]
async fn a_run_whose_store_fails_after_the_start_shows_no_credential_in_logs_metrics_or_cluster_writes()
 {
    let (logs, _guard) = testing::logs::capture();
    let mut answers = vec![canned(200, listing(&[]))];
    answers.extend(refusals(50));
    let stub = Stub::serving(answers).await;
    let kube = Arc::new(FakeKube::default());
    kube.set_namespaces(&["sbx"]);
    kube.add_replica_set("sbx", "warm-p", Some(2));
    kube.put_pod(PodFacts {
        phase: Phase::Pending,
        ready: false,
        containers: vec![fixtures::waiting("ErrImagePull")],
        ..fixtures::warm("w0", START - 10)
    });
    let clock: Arc<dyn Clock> = Arc::new(TokioClock::starting_at(at(START)));
    let parts = Parts::new(
        kube.clone(),
        Arc::new(bucket_of(&stub)),
        clock,
        "a".to_owned(),
        "keeper-ns".to_owned(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let mut document = testing::document();
    document["sweep_interval_seconds"] = 1.into();
    let config = testing::config_of(&document);
    let handle = tokio::spawn(run_on(listener, config, parts, shutdown.clone()));
    let mut metrics = String::new();
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        metrics = request(address, "GET", "/metrics").await.2;
        if !metrics.contains("br_sandbox_keeper_sweep_errors_total 0") && !metrics.is_empty() {
            break;
        }
    }
    shutdown.cancel();
    assert_eq!(handle.await.unwrap(), Ok(()));
    assert!(
        !metrics.contains("br_sandbox_keeper_sweep_errors_total 0"),
        "{metrics}"
    );
    let text = logs.text();
    assert!(text.contains("the listing failed"), "{text}");
    assert!(holds_no_secret(&text), "{text}");
    assert!(holds_no_secret(&metrics), "{metrics}");
    assert!(holds_no_secret(&format!("{:?}", kube.calls())));
}
