use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::*;

struct Running {
    kube: Arc<FakeKube>,
    leadership: Arc<Leadership>,
    shutdown: CancellationToken,
    handle: JoinHandle<()>,
}

fn launch(bed: Bed) -> Running {
    let Bed {
        kube,
        engine,
        notices,
        leadership,
        ..
    } = bed;
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let handle = tokio::spawn(engine.run(notices, stop));
    Running {
        kube,
        leadership,
        shutdown,
        handle,
    }
}

impl Running {
    async fn stop(self) {
        self.shutdown.cancel();
        self.handle.await.unwrap();
    }
}

async fn sleep(seconds: u64) {
    tokio::time::sleep(Duration::from_secs(seconds)).await;
}

#[tokio::test(start_paused = true)]
async fn the_run_loop_applies_watch_events_and_the_deadlines_and_stops_at_shutdown() {
    let bed = bed(true);
    bed.kube.put_pod(old_warm("w0", 1700));
    let running = launch(bed);
    sleep(1).await;
    assert!(running.kube.pod(NS, "w0").is_some());
    sleep(150).await;
    assert!(running.kube.pod(NS, "w0").is_none());
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn the_run_loop_judges_again_when_the_leadership_is_gained() {
    let bed = bed(false);
    bed.kube.put_pod(old_warm("w0", 5000));
    let running = launch(bed);
    sleep(5).await;
    assert!(running.kube.pod(NS, "w0").is_some());
    running.leadership.renewed(at(START + 1_000_000));
    sleep(1).await;
    assert!(running.kube.pod(NS, "w0").is_none());
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_namespace_that_gets_the_managed_label_after_the_start_is_served_at_once() {
    let running = launch(bed(true));
    sleep(5).await;
    running.kube.put_namespace(managed("late"));
    sleep(1).await;
    running.kube.put_pod(crate::facts::PodFacts {
        namespace: "late".to_owned(),
        pool_id: Some("ghost".to_owned()),
        ..old_warm("late-warm", 5000)
    });
    sleep(5).await;
    assert!(running.kube.pod("late", "late-warm").is_none());
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_namespace_that_loses_the_managed_label_is_no_longer_served() {
    let bed = bed(true);
    bed.kube.put_pod(old_warm("w0", 1000));
    let running = launch(bed);
    sleep(5).await;
    assert!(!running.kube.calls().is_empty());
    running.kube.remove_namespace(NS);
    sleep(5).await;
    running.kube.clear_calls();
    sleep(1000).await;
    assert!(running.kube.pod(NS, "w0").is_some());
    assert!(running.kube.calls().is_empty());
    assert!(!running.handle.is_finished());
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_namespace_that_gets_the_label_back_is_served_again_from_a_new_sync() {
    let bed = bed(true);
    bed.kube.put_pod(old_warm("w0", 1000));
    let running = launch(bed);
    sleep(5).await;
    running.kube.remove_namespace(NS);
    sleep(1000).await;
    assert!(running.kube.pod(NS, "w0").is_some());
    running.kube.put_namespace(managed(NS));
    sleep(5).await;
    assert!(running.kube.pod(NS, "w0").is_none());
    running.stop().await;
}
