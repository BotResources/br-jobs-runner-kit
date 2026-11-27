use tokio_util::sync::CancellationToken;

use super::*;

#[tokio::test(start_paused = true)]
async fn the_run_loop_applies_watch_events_and_the_deadlines_and_stops_at_shutdown() {
    let Bed {
        kube,
        engine,
        readiness,
        ..
    } = bed(true);
    readiness.store_ready();
    kube.put_pod(old_warm("w0", 1700));
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let handle = tokio::spawn(async move { engine.run(&[NS.to_owned()], stop).await });
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(readiness.is_ready());
    assert!(kube.pod(NS, "w0").is_some());
    tokio::time::sleep(Duration::from_secs(150)).await;
    assert!(kube.pod(NS, "w0").is_none());
    shutdown.cancel();
    handle.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn the_run_loop_judges_again_when_the_leadership_is_gained() {
    let Bed {
        kube,
        engine,
        leadership,
        ..
    } = bed(false);
    kube.put_pod(old_warm("w0", 5000));
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let handle = tokio::spawn(async move { engine.run(&[NS.to_owned()], stop).await });
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(kube.pod(NS, "w0").is_some());
    leadership.renewed(at(START + 1_000_000));
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(kube.pod(NS, "w0").is_none());
    shutdown.cancel();
    handle.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_namespace_that_gets_the_managed_label_after_the_start_is_served_at_the_next_discovery() {
    let Bed { kube, engine, .. } = bed(true);
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let handle = tokio::spawn(async move { engine.run(&[NS.to_owned()], stop).await });
    tokio::time::sleep(Duration::from_secs(5)).await;
    kube.set_namespaces(&[NS, "late"]);
    kube.put_pod(crate::facts::PodFacts {
        namespace: "late".to_owned(),
        pool_id: Some("ghost".to_owned()),
        ..old_warm("late-warm", 5000)
    });
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(kube.pod("late", "late-warm").is_some());
    tokio::time::sleep(Duration::from_secs(40)).await;
    assert!(kube.pod("late", "late-warm").is_none());
    shutdown.cancel();
    handle.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_namespace_that_loses_the_managed_label_is_no_longer_served_at_the_next_discovery() {
    let Bed { kube, engine, .. } = bed(true);
    kube.put_pod(old_warm("w0", 1000));
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let handle = tokio::spawn(async move { engine.run(&[NS.to_owned()], stop).await });
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(!kube.calls().is_empty());
    kube.set_namespaces(&[]);
    tokio::time::sleep(Duration::from_secs(70)).await;
    kube.clear_calls();
    tokio::time::sleep(Duration::from_secs(1000)).await;
    assert!(kube.pod(NS, "w0").is_some());
    assert!(kube.calls().is_empty());
    assert!(!handle.is_finished());
    shutdown.cancel();
    handle.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_namespace_that_gets_the_label_back_is_served_again_from_a_new_sync() {
    let Bed { kube, engine, .. } = bed(true);
    kube.put_pod(old_warm("w0", 1000));
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let handle = tokio::spawn(async move { engine.run(&[NS.to_owned()], stop).await });
    tokio::time::sleep(Duration::from_secs(5)).await;
    kube.set_namespaces(&[]);
    tokio::time::sleep(Duration::from_secs(70)).await;
    tokio::time::sleep(Duration::from_secs(1000)).await;
    assert!(kube.pod(NS, "w0").is_some());
    kube.set_namespaces(&[NS]);
    tokio::time::sleep(Duration::from_secs(70)).await;
    assert!(kube.pod(NS, "w0").is_none());
    shutdown.cancel();
    handle.await.unwrap();
}
