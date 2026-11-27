use crate::facts::{Mark, Phase, PodFacts, fixtures};

use super::*;

const STALE: &str = "stale";

fn with_step(bed: &mut Bed, step: i64) {
    let mut document = testing::document();
    document["scale_down_step"] = step.into();
    bed.engine.config = Arc::new(testing::config_of(&document));
}

fn stale(name: &str, age: u64, ready: bool) -> PodFacts {
    PodFacts {
        ready,
        template_hash: Mark::Value(STALE.to_owned()),
        ..old_warm(name, age)
    }
}

fn current(bed: &Bed, name: &str, ready: bool) -> PodFacts {
    PodFacts {
        ready,
        template_hash: Mark::Value(bed.template_hash().unwrap()),
        ..old_warm(name, 5)
    }
}

async fn stale_bed(step: i64) -> Bed {
    let mut bed = bed(true);
    bed.kube.put_replica_set(owned_replica_set(NS, 4));
    with_step(&mut bed, step);
    for (name, age, ready) in [
        ("w0", 40, true),
        ("w1", 30, false),
        ("w2", 20, true),
        ("w3", 10, true),
    ] {
        bed.kube.put_pod(stale(name, age, ready));
    }
    bed.sync().await;
    bed
}

#[tokio::test(start_paused = true)]
async fn a_step_removes_at_most_the_step_size_of_stale_pods_not_ready_first_then_the_oldest() {
    let bed = stale_bed(2).await;
    assert_eq!(bed.deletes(), vec!["w1".to_owned(), "w0".to_owned()]);
    assert!(bed.has_metric("br_sandbox_keeper_pods_deleted_total{reason=\"rollout\"} 2"));
    assert!(bed.kube.pod(NS, "w2").is_some());
    assert!(bed.kube.pod(NS, "w3").is_some());
}

#[tokio::test(start_paused = true)]
async fn the_next_step_waits_until_every_warm_pod_of_the_current_template_is_ready() {
    let mut bed = stale_bed(2).await;
    bed.kube.put_pod(current(&bed, "r0", false));
    bed.kube.put_pod(current(&bed, "r1", true));
    bed.kube.clear_calls();
    let bucket = bed.bucket();
    bed.pass(bucket * 3).await;
    assert!(bed.deletes().is_empty());
    bed.kube.update_pod(NS, "r0", |pod| pod.ready = true);
    bed.pass(bucket + 1).await;
    assert_eq!(bed.deletes(), vec!["w2".to_owned(), "w3".to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn the_rollout_stops_while_a_warm_pod_of_the_current_template_fails() {
    let mut bed = stale_bed(2).await;
    bed.kube.put_pod(PodFacts {
        phase: Phase::Pending,
        containers: vec![fixtures::waiting("ErrImagePull")],
        ..current(&bed, "r0", false)
    });
    bed.kube.put_pod(current(&bed, "r1", true));
    bed.kube.clear_calls();
    let bucket = bed.bucket();
    bed.pass(bucket * 3).await;
    assert!(bed.deletes().is_empty());
    assert!(bed.kube.pod(NS, "w2").is_some());
    assert!(bed.kube.pod(NS, "w3").is_some());
    assert_eq!(
        bed.condition("WarmPodsFailing"),
        Some(("True".to_owned(), "ImagePullFailing".to_owned()))
    );
}

#[tokio::test(start_paused = true)]
async fn a_stopped_rollout_never_keeps_a_stale_pod_past_the_warm_max_age() {
    let mut bed = bed(true);
    bed.kube.put_replica_set(owned_replica_set(NS, 3));
    with_step(&mut bed, 2);
    bed.kube.put_pod(stale("w0", 1790, true));
    bed.kube.put_pod(stale("w1", 10, true));
    bed.sync().await;
    bed.kube.clear_calls();
    bed.kube.put_pod(PodFacts {
        phase: Phase::Pending,
        containers: vec![fixtures::waiting("ErrImagePull")],
        ..current(&bed, "r0", false)
    });
    bed.pass(30).await;
    assert!(bed.kube.pod(NS, "w0").is_none());
    assert!(bed.kube.pod(NS, "w1").is_some());
    assert!(bed.has_metric("br_sandbox_keeper_pods_deleted_total{reason=\"warm_max_age\"} 1"));
}

#[tokio::test(start_paused = true)]
async fn the_rollout_waits_while_the_pool_has_fewer_warm_pods_than_its_replicas() {
    let mut bed = bed(true);
    bed.kube.put_pod(stale("w0", 40, true));
    bed.sync().await;
    bed.pass(60).await;
    assert!(bed.deletes().is_empty());
    assert!(bed.kube.pod(NS, "w0").is_some());
}

#[tokio::test(start_paused = true)]
async fn a_claimed_pod_is_never_deleted_by_the_rollout() {
    let mut bed = bed(true);
    bed.kube.put_pod(PodFacts {
        template_hash: Mark::Value(STALE.to_owned()),
        claimed_at: Mark::Value(at(START)),
        ..fixtures::claimed("c0", false, START)
    });
    bed.kube.put_pod(stale("w0", 40, true));
    bed.kube.put_pod(stale("w1", 30, true));
    bed.sync().await;
    assert!(bed.kube.pod(NS, "c0").is_some());
    assert!(!bed.deletes().contains(&"c0".to_owned()));
}

#[tokio::test(start_paused = true)]
async fn a_follower_deletes_no_pod_for_a_rollout() {
    let mut bed = bed(false);
    bed.kube.put_pod(stale("w0", 40, true));
    bed.kube.put_pod(stale("w1", 30, true));
    bed.sync().await;
    bed.pass(60).await;
    assert!(bed.kube.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_pod_of_the_current_template_is_never_stale_and_updated_replicas_counts_them() {
    let mut bed = stale_bed(2).await;
    bed.kube.put_pod(current(&bed, "r0", true));
    bed.kube.put_pod(current(&bed, "r1", true));
    let bucket = bed.bucket();
    bed.pass(bucket * 2).await;
    assert!(bed.kube.pod(NS, "r0").is_some());
    assert!(bed.kube.pod(NS, "r1").is_some());
    let status = bed.status().unwrap();
    assert_eq!(status["updatedReplicas"], 2);
}

#[tokio::test(start_paused = true)]
async fn a_conflict_on_a_stale_pod_that_a_claim_changed_is_judged_again_as_a_claimed_pod() {
    let mut bed = bed(true);
    with_step(&mut bed, 2);
    bed.kube.put_pod(stale("w0", 40, true));
    bed.kube.put_pod(stale("w1", 30, true));
    bed.kube.silent_update_pod(NS, "w0", |pod| {
        pod.pool_state = crate::facts::PoolState::Claimed;
        pod.owned = false;
        pod.clean = Mark::Value(false);
        pod.claimed_at = Mark::Value(at(START));
    });
    bed.sync().await;
    assert!(bed.kube.pod(NS, "w0").is_some());
}
