use crate::facts::{Mark, Phase, PodFacts, fixtures};

use super::*;

fn idle_pod(name: &str, idle_since: u64, state_bytes: u64) -> PodFacts {
    PodFacts {
        idle_since: Mark::Value(at(idle_since)),
        state_bytes: Mark::Value(state_bytes),
        ..fixtures::claimed(name, true, START - 500)
    }
}

#[tokio::test(start_paused = true)]
async fn an_old_warm_pod_is_deleted_with_the_uid_and_the_resource_version_it_was_judged_in() {
    let mut bed = bed(true);
    let pod = bed.kube.put_pod(old_warm("w0", 2000));
    bed.applied(pod.clone()).await;
    assert_eq!(
        bed.kube.calls()[0],
        Call::Delete {
            name: "w0".to_owned(),
            uid: pod.uid.clone(),
            resource_version: pod.resource_version.clone(),
        }
    );
    assert!(bed.kube.pod(NS, "w0").is_none());
    assert!(bed.has_metric("br_sandbox_keeper_pods_deleted_total{reason=\"warm_max_age\"} 1"));
}

#[tokio::test(start_paused = true)]
async fn a_warm_pod_is_deleted_when_the_deadline_comes_and_not_before() {
    let mut bed = bed(true);
    let pod = bed.kube.put_pod(old_warm("w0", 1700));
    bed.applied(pod).await;
    bed.pass(99).await;
    assert!(bed.deletes().is_empty());
    bed.pass(2).await;
    assert_eq!(bed.deletes(), vec!["w0".to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn a_delete_that_races_a_claim_ends_in_a_conflict_then_the_pod_is_read_again_and_kept() {
    let mut bed = bed(true);
    let pod = bed.kube.put_pod(idle_pod("c0", START - 1000, 0));
    bed.kube.silent_update_pod(NS, "c0", |pod| {
        pod.clean = Mark::Value(false);
        pod.claimed_at = Mark::Value(at(START));
        pod.idle_since = Mark::Absent;
    });
    bed.applied(pod).await;
    assert_eq!(bed.deletes(), vec!["c0".to_owned()]);
    assert!(bed.kube.pod(NS, "c0").is_some());
    assert!(bed.has_metric("br_sandbox_keeper_pods_deleted_total{reason=\"idle\"} 0"));
}

#[tokio::test(start_paused = true)]
async fn a_pod_that_is_already_gone_is_not_an_error() {
    let mut bed = bed(true);
    let pod = bed.kube.put_pod(old_warm("w0", 2000));
    bed.kube.forget_pod(NS, "w0");
    bed.applied(pod).await;
    assert!(bed.has_metric("br_sandbox_keeper_sweep_errors_total 0"));
    assert!(bed.engine.pods.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_failed_delete_is_counted_and_tried_again() {
    let mut bed = bed(true);
    let pod = bed.kube.put_pod(old_warm("w0", 2000));
    bed.kube.fail("delete_pod");
    bed.applied(pod).await;
    assert!(bed.has_metric("br_sandbox_keeper_sweep_errors_total 1"));
    bed.kube.heal("delete_pod");
    bed.pass(3).await;
    assert!(bed.kube.pod(NS, "w0").is_none());
}

#[tokio::test(start_paused = true)]
async fn a_clean_pod_below_the_threshold_goes_at_its_idle_since_and_above_it_after_the_idle_ttl() {
    let mut bed = bed(true);
    let small = bed.kube.put_pod(idle_pod("small", START, 10));
    let large = bed.kube.put_pod(idle_pod("large", START, 50_000_000));
    bed.applied(small).await;
    assert!(bed.kube.pod(NS, "small").is_none());
    bed.applied(large).await;
    assert!(bed.kube.pod(NS, "large").is_some());
    bed.pass(119).await;
    assert!(bed.kube.pod(NS, "large").is_some());
    bed.pass(2).await;
    assert!(bed.kube.pod(NS, "large").is_none());
    assert!(bed.has_metric("br_sandbox_keeper_pods_deleted_total{reason=\"idle\"} 2"));
}

#[tokio::test(start_paused = true)]
async fn a_dirty_claimed_pod_goes_at_the_stuck_cap() {
    let mut bed = bed(true);
    let pod = bed.kube.put_pod(PodFacts {
        claimed_at: Mark::Value(at(START - 7000)),
        ..fixtures::claimed("d0", false, START - 7000)
    });
    bed.applied(pod).await;
    assert!(bed.kube.pod(NS, "d0").is_some());
    bed.pass(201).await;
    assert!(bed.kube.pod(NS, "d0").is_none());
    assert!(bed.has_metric("br_sandbox_keeper_pods_deleted_total{reason=\"stuck\"} 1"));
}

#[tokio::test(start_paused = true)]
async fn a_pod_with_a_missing_annotation_is_never_deleted_by_that_rule_and_is_counted() {
    let mut bed = bed(true);
    let pod = bed.kube.put_pod(PodFacts {
        clean: Mark::Absent,
        ..fixtures::claimed("u0", true, START - 100_000)
    });
    bed.applied(pod).await;
    bed.pass(30).await;
    assert!(bed.kube.pod(NS, "u0").is_some());
    assert!(bed.has_metric("br_sandbox_keeper_unreadable_pods 1"));
}

#[tokio::test(start_paused = true)]
async fn a_failed_pod_goes_after_the_ttl_counted_from_the_first_sight() {
    let mut bed = bed(true);
    let pod = bed.kube.put_pod(PodFacts {
        phase: Phase::Failed,
        ready: false,
        ..old_warm("f0", 100)
    });
    bed.applied(pod).await;
    bed.pass(299).await;
    assert!(bed.kube.pod(NS, "f0").is_some());
    bed.pass(2).await;
    assert!(bed.kube.pod(NS, "f0").is_none());
    assert!(bed.has_metric("br_sandbox_keeper_pods_deleted_total{reason=\"failed\"} 1"));
}

#[tokio::test(start_paused = true)]
async fn a_follower_deletes_nothing() {
    let mut bed = bed(false);
    let pod = bed.kube.put_pod(old_warm("w0", 2000));
    bed.applied(pod).await;
    bed.pass(10).await;
    assert!(bed.kube.calls().is_empty());
    assert!(bed.kube.pod(NS, "w0").is_some());
}

#[tokio::test(start_paused = true)]
async fn the_event_warm_pods_pending_is_raised_once_for_each_pod_and_never_for_a_failing_pod() {
    let mut bed = bed(true);
    bed.sync().await;
    let pending = bed.kube.put_pod(PodFacts {
        phase: Phase::Pending,
        ready: false,
        ..old_warm("p0", 100)
    });
    let failing = bed.kube.put_pod(PodFacts {
        phase: Phase::Pending,
        ready: false,
        containers: vec![fixtures::waiting("ImagePullBackOff")],
        ..old_warm("p1", 100)
    });
    bed.applied(pending.clone()).await;
    bed.applied(failing).await;
    bed.pass(30).await;
    bed.applied(pending).await;
    let events: Vec<_> = bed
        .kube
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Event {
                reason: "WarmPodsPending",
                message,
            } => Some(message),
            _ => None,
        })
        .collect();
    assert_eq!(events.len(), 1);
    assert!(events[0].contains("p0"));
}

#[tokio::test(start_paused = true)]
async fn a_deleted_pod_event_forgets_the_pod() {
    let mut bed = bed(true);
    bed.sync().await;
    let pod = bed.kube.put_pod(old_warm("w0", 10));
    bed.applied(pod).await;
    assert!(bed.has_metric("br_sandbox_keeper_pool_warm_ready{pool=\"p\"} 1"));
    bed.engine
        .handle(
            NS,
            PodEvent::Deleted {
                name: "w0".to_owned(),
            },
        )
        .await;
    assert!(bed.has_metric("br_sandbox_keeper_pool_warm_ready{pool=\"p\"} 0"));
}

#[tokio::test(start_paused = true)]
async fn a_leader_whose_lease_passed_the_renew_deadline_refuses_every_write() {
    let mut bed = bed(false);
    bed.leadership.renewed(at(START + 5));
    let pod = bed.kube.put_pod(old_warm("w0", 2000));
    tokio::time::advance(Duration::from_secs(10)).await;
    bed.applied(pod).await;
    bed.engine.evaluate_all().await;
    assert!(bed.kube.calls().is_empty());
    assert!(bed.kube.pod(NS, "w0").is_some());
}
