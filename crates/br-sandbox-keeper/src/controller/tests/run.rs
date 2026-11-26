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
    let events = Engine::watches(kube.as_ref(), &[NS.to_owned()]);
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(engine.run(events, shutdown.clone()));
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
    let events = Engine::watches(kube.as_ref(), &[NS.to_owned()]);
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(engine.run(events, shutdown.clone()));
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(kube.pod(NS, "w0").is_some());
    leadership.renewed(at(START + 1_000_000));
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(kube.pod(NS, "w0").is_none());
    shutdown.cancel();
    handle.await.unwrap();
}
