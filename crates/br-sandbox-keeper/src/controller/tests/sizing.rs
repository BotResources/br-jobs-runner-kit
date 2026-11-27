use crate::cluster::PoolEvent;
use crate::facts::{PoolState, fixtures};

use super::*;

fn claim(bed: &Bed, name: &str) -> crate::facts::PodFacts {
    let warm = bed.kube.pod(NS, name).unwrap();
    bed.kube.update_pod(NS, name, |pod| {
        pod.pool_state = PoolState::Claimed;
        pod.owned = false;
    });
    PodFacts {
        pool_state: PoolState::Claimed,
        owned: false,
        ..warm
    }
}

use crate::facts::PodFacts;

async fn bed_with_warm(count: usize) -> Bed {
    let mut bed = bed(true);
    for index in 0..count {
        bed.kube.put_pod(old_warm(&format!("w{index}"), 10));
    }
    bed.sync().await;
    bed.kube.clear_calls();
    bed
}

#[tokio::test(start_paused = true)]
async fn claims_raise_the_replicas_at_once_to_the_sized_value() {
    let mut bed = bed_with_warm(40).await;
    for index in 0..40 {
        let claimed = claim(&bed, &format!("w{index}"));
        bed.applied(claimed).await;
    }
    assert_eq!(bed.replica_writes().last(), Some(&14));
    assert_eq!(bed.replicas(), Some(14));
    assert!(bed.has_metric("br_sandbox_keeper_pool_desired{pool=\"p\"} 14"));
}

#[tokio::test(start_paused = true)]
async fn the_replicas_are_written_only_when_they_differ() {
    let mut bed = bed_with_warm(3).await;
    bed.kube.add_replica_set(NS, RS, Some(2));
    bed.engine.evaluate_all().await;
    assert!(bed.replica_writes().is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_rate_is_the_peak_of_a_bucket_and_not_the_mean_of_the_window() {
    let mut bed = bed_with_warm(10).await;
    for index in 0..10 {
        let claimed = claim(&bed, &format!("w{index}"));
        bed.applied(claimed).await;
    }
    assert!(bed.has_metric("br_sandbox_keeper_claims_per_second{pool=\"p\"} 1"));
}

#[tokio::test(start_paused = true)]
async fn a_cold_pod_created_claimed_counts_once_and_a_pod_seen_in_a_snapshot_does_not() {
    let mut bed = bed(true);
    bed.sync().await;
    let cold = PodFacts {
        created_at: at(START),
        ..fixtures::claimed("cold", true, START)
    };
    bed.applied(cold.clone()).await;
    bed.applied(cold).await;
    assert!(bed.has_metric("br_sandbox_keeper_claims_per_second{pool=\"p\"} 0.1"));
    let mut other = crate::controller::tests::bed(true);
    let seen = PodFacts {
        created_at: at(START),
        ..fixtures::claimed("seen", true, START)
    };
    other
        .engine
        .handle(NS, PodEvent::Snapshot(vec![seen]))
        .await;
    assert!(other.has_metric("br_sandbox_keeper_claims_per_second{pool=\"p\"} 0"));
}

#[tokio::test(start_paused = true)]
async fn an_owned_claimed_pod_or_an_old_unseen_one_is_not_a_cold_claim() {
    let mut bed = bed(true);
    bed.sync().await;
    let owned = PodFacts {
        owned: true,
        created_at: at(START),
        ..fixtures::claimed("owned", true, START)
    };
    let old = PodFacts {
        created_at: at(START - 10_000),
        ..fixtures::claimed("old", true, START)
    };
    bed.applied(owned).await;
    bed.applied(old).await;
    assert!(bed.has_metric("br_sandbox_keeper_claims_per_second{pool=\"p\"} 0"));
}

#[tokio::test(start_paused = true)]
async fn a_scale_down_waits_for_the_cooldown_then_falls_by_steps_and_reaches_the_size() {
    let mut bed = bed(true);
    bed.kube.add_replica_set(NS, RS, Some(20));
    bed.sync().await;
    let mut reached = None;
    for second in 1..=400u64 {
        bed.pass(1).await;
        if bed.replicas() == Some(2) && reached.is_none() {
            reached = Some(second);
        }
    }
    assert_eq!(bed.replica_writes(), vec![15, 10, 5, 2]);
    let reached = reached.unwrap();
    assert!((150..=160).contains(&reached), "{reached}");
}

#[tokio::test(start_paused = true)]
async fn the_floor_of_the_day_holds_the_replicas_up() {
    let mut bed = bed(true);
    bed.engine.pools[0].settings.floors = vec![crate::pool::FloorWindow::new(0, 86_400, 9)];
    bed.sync().await;
    assert_eq!(bed.replicas(), Some(9));
}

#[tokio::test(start_paused = true)]
async fn the_event_pool_at_max_is_raised_once_for_each_transition_into_the_state() {
    let mut bed = bed_with_warm(120).await;
    for index in 0..120 {
        let claimed = claim(&bed, &format!("w{index}"));
        bed.applied(claimed).await;
    }
    let events = |bed: &Bed| {
        bed.kube
            .calls()
            .into_iter()
            .filter(|call| matches!(call, Call::Event { reason, .. } if *reason == PoolEvent::AtMax.reason()))
            .count()
    };
    assert_eq!(events(&bed), 1);
    assert_eq!(bed.replicas(), Some(20));
    bed.pass(400).await;
    assert_eq!(events(&bed), 1);
}

#[tokio::test(start_paused = true)]
async fn only_the_leader_resizes_the_pool_and_a_new_leader_does_at_once() {
    let mut bed = bed(false);
    for index in 0..40 {
        bed.kube.put_pod(old_warm(&format!("w{index}"), 10));
    }
    bed.sync().await;
    for index in 0..40 {
        let claimed = claim(&bed, &format!("w{index}"));
        bed.applied(claimed).await;
    }
    assert!(bed.kube.calls().is_empty());
    bed.leadership.renewed(at(START + 1_000_000));
    bed.engine.gained_leadership().await;
    assert_eq!(bed.replicas(), Some(14));
}

#[tokio::test(start_paused = true)]
async fn a_failed_write_of_the_replicas_is_counted_and_tried_again_after_the_retry_period() {
    let mut bed = bed_with_warm(40).await;
    bed.kube.fail("apply_replicas");
    for index in 0..40 {
        let claimed = claim(&bed, &format!("w{index}"));
        bed.applied(claimed).await;
    }
    assert!(bed.has_metric("br_sandbox_keeper_sweep_errors_total 40"));
    assert_eq!(bed.replicas(), Some(2));
    bed.kube.heal("apply_replicas");
    bed.pass(3).await;
    assert_eq!(bed.replicas(), Some(14));
}

fn no_wake_up_is_late(bed: &Bed) -> bool {
    let now = bed.engine.clock.now();
    bed.engine.pool_wake.values().all(|at| !at.has_passed(now))
}

#[tokio::test(start_paused = true)]
async fn a_replica_that_lost_the_lease_keeps_no_wake_up_that_has_passed() {
    let mut bed = bed(true);
    bed.kube.add_replica_set(NS, RS, Some(20));
    bed.sync().await;
    assert!(!bed.engine.pool_wake.is_empty());
    bed.leadership.lost();
    bed.pass(300).await;
    assert!(no_wake_up_is_late(&bed));
    assert!(bed.replica_writes().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_replica_that_never_led_keeps_no_wake_up_that_has_passed() {
    let mut bed = bed(false);
    bed.kube.add_replica_set(NS, RS, Some(20));
    bed.sync().await;
    bed.pass(300).await;
    assert!(no_wake_up_is_late(&bed));
}

#[tokio::test(start_paused = true)]
async fn a_pool_whose_replicaset_is_gone_keeps_no_wake_up_that_has_passed() {
    let mut bed = bed(true);
    bed.kube.add_replica_set(NS, RS, Some(20));
    bed.sync().await;
    assert!(!bed.engine.pool_wake.is_empty());
    bed.kube.remove_replica_set(NS, RS);
    bed.pass(300).await;
    assert!(no_wake_up_is_late(&bed));
}

#[tokio::test(start_paused = true)]
async fn a_leader_that_lost_the_lease_and_gets_it_back_goes_on_with_the_scale_down_at_once() {
    let mut bed = bed(true);
    bed.kube.add_replica_set(NS, RS, Some(20));
    bed.sync().await;
    bed.leadership.lost();
    bed.pass(300).await;
    assert!(bed.replica_writes().is_empty());
    bed.leadership.renewed(at(START + 1_000_000));
    bed.engine.gained_leadership().await;
    assert_eq!(bed.replica_writes(), vec![15]);
}
