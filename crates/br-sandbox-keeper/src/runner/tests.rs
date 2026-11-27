use std::time::Duration;

use br_runner_sandbox::Clock;

use super::*;
use crate::bucket::BucketError;
use crate::facts::fixtures;
use crate::fakes::{FakeBucket, FakeKube, TokioClock};
use crate::http::tests::request;
use crate::leader::LEASE_NAME;
use crate::testing;
use crate::time::at;

const START: u64 = 1_000_000;

struct Rig {
    kube: Arc<FakeKube>,
    bucket: Arc<FakeBucket>,
}

fn rig() -> Rig {
    let kube = Arc::new(FakeKube::default());
    kube.set_namespaces(&["sbx"]);
    kube.add_replica_set("sbx", "warm-p", Some(2));
    Rig {
        kube,
        bucket: Arc::new(FakeBucket::default()),
    }
}

fn parts(rig: &Rig, identity: &str) -> Parts {
    let clock: Arc<dyn Clock> = Arc::new(TokioClock::starting_at(at(START)));
    Parts {
        api: rig.kube.clone(),
        bucket: rig.bucket.clone(),
        clock,
        identity: identity.to_owned(),
        own_namespace: "keeper-ns".to_owned(),
    }
}

async fn listener() -> (TcpListener, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    (listener, address)
}

async fn start_error(rig: &Rig) -> KeeperError {
    let (listener, _) = listener().await;
    run_on(
        listener,
        testing::config(),
        parts(rig, "a"),
        CancellationToken::new(),
    )
    .await
    .unwrap_err()
}

#[tokio::test(start_paused = true)]
async fn it_does_not_start_when_the_api_is_unreachable() {
    let rig = rig();
    rig.kube.fail("namespaces");
    assert!(matches!(
        start_error(&rig).await,
        KeeperError::ApiUnreachable(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn it_does_not_start_when_a_pool_namespace_is_not_managed() {
    let rig = rig();
    rig.kube.set_namespaces(&["other"]);
    assert_eq!(
        start_error(&rig).await,
        KeeperError::PoolNamespaceNotManaged {
            pool: "p".to_owned(),
            namespace: "sbx".to_owned()
        }
    );
}

#[tokio::test(start_paused = true)]
async fn it_does_not_start_when_the_bucket_is_absent_and_never_creates_it() {
    let rig = rig();
    rig.bucket.fail_check(BucketError::Absent);
    assert_eq!(
        start_error(&rig).await,
        KeeperError::BucketAbsent("saves".to_owned())
    );
    assert!(rig.bucket.writes().is_empty());
}

#[tokio::test(start_paused = true)]
async fn it_does_not_start_when_the_store_is_unreachable() {
    let rig = rig();
    rig.bucket
        .fail_check(BucketError::Unreachable("no route".to_owned()));
    assert!(matches!(
        start_error(&rig).await,
        KeeperError::StoreUnreachable(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn it_does_not_start_on_an_address_that_cannot_be_bound() {
    let rig = rig();
    let mut document = testing::document();
    let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
    document["listen_address"] = taken.local_addr().unwrap().to_string().into();
    let error = run(
        testing::config_of(&document),
        parts(&rig, "a"),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, KeeperError::Listen { .. }));
}

#[tokio::test(start_paused = true)]
async fn the_leader_serves_the_health_endpoints_expires_pods_and_saves_and_stops_on_the_signal() {
    let rig = rig();
    rig.kube.put_pod(fixtures::warm("w0", START - 5000));
    let (listener, address) = listener().await;
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(run_on(
        listener,
        testing::config(),
        parts(&rig, "a"),
        shutdown.clone(),
    ));
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(request(address, "GET", "/livez").await.0, 200);
    assert_eq!(request(address, "GET", "/readyz").await.0, 200);
    let metrics = request(address, "GET", "/metrics").await.2;
    assert!(metrics.contains("br_sandbox_keeper_is_leader 1"));
    assert!(metrics.contains("br_sandbox_keeper_pods_deleted_total{reason=\"warm_max_age\"} 1"));
    assert!(rig.kube.pod("sbx", "w0").is_none());
    assert_eq!(
        rig.kube.lease().and_then(|lease| lease.holder),
        Some("a".to_owned())
    );
    shutdown.cancel();
    assert_eq!(handle.await.unwrap(), Ok(()));
}

#[tokio::test(start_paused = true)]
async fn every_managed_namespace_is_served_also_one_with_no_configured_pool() {
    let rig = rig();
    rig.kube.set_namespaces(&["sbx", "other"]);
    rig.kube.put_pod(crate::facts::PodFacts {
        namespace: "other".to_owned(),
        pool_id: Some("ghost".to_owned()),
        ..fixtures::warm("g0", START - 5000)
    });
    rig.kube.put_pod(crate::facts::PodFacts {
        namespace: "other".to_owned(),
        pool_id: Some("ghost".to_owned()),
        ..fixtures::warm("g1", START - 10)
    });
    let (listener, address) = listener().await;
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(run_on(
        listener,
        testing::config(),
        parts(&rig, "a"),
        shutdown.clone(),
    ));
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(request(address, "GET", "/readyz").await.0, 200);
    assert!(rig.kube.pod("other", "g0").is_none());
    assert!(rig.kube.pod("other", "g1").is_some());
    shutdown.cancel();
    assert_eq!(handle.await.unwrap(), Ok(()));
}

#[tokio::test(start_paused = true)]
async fn a_follower_keeps_its_watches_ready_and_writes_nothing() {
    let rig = rig();
    rig.kube.set_lease(Some(crate::cluster::LeaseFacts {
        holder: Some("other".to_owned()),
        acquire_time: Some(at(START)),
        renew_time: Some(at(START)),
        duration: Some(Duration::from_secs(1_000_000)),
        transitions: 1,
        resource_version: "9".to_owned(),
    }));
    rig.kube.put_pod(fixtures::warm("w0", START - 5000));
    let (listener, address) = listener().await;
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(run_on(
        listener,
        testing::config(),
        parts(&rig, "b"),
        shutdown.clone(),
    ));
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert_eq!(request(address, "GET", "/readyz").await.0, 200);
    let metrics = request(address, "GET", "/metrics").await.2;
    assert!(metrics.contains("br_sandbox_keeper_is_leader 0"));
    assert!(rig.kube.pod("sbx", "w0").is_some());
    assert!(rig.kube.calls().is_empty());
    assert!(rig.bucket.writes().is_empty());
    assert_eq!(LEASE_NAME, "br-sandbox-keeper");
    shutdown.cancel();
    assert_eq!(handle.await.unwrap(), Ok(()));
}
