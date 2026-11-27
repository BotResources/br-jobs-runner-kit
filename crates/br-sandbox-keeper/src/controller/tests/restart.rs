use crate::resource::FloorSpec;

use super::*;

fn floored() -> SandboxPool {
    let mut spec = spec();
    spec.floors = vec![FloorSpec::new("00:00", "24:00", 9)];
    resource("main", "uid-1", START - 100, &spec)
}

async fn busy_pool() -> Bed {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, floored());
    bed.kube.put_replica_set(owned_replica_set(NS, 2));
    for index in 0..40 {
        bed.kube.put_pod(old_warm(&format!("w{index}"), 10));
    }
    bed.sync().await;
    for index in 0..40 {
        bed.kube.update_pod(NS, &format!("w{index}"), |pod| {
            pod.pool_state = crate::facts::PoolState::Claimed;
            pod.owned = false;
        });
    }
    bed.settle().await;
    bed
}

#[tokio::test(start_paused = true)]
async fn a_restarted_replica_starts_from_the_floor_and_deletes_and_writes_nothing() {
    let busy = busy_pool().await;
    assert_eq!(busy.replicas(), Some(14));
    assert!(busy.has_metric("br_sandbox_keeper_pool_desired{pool=\"sbx\"} 14"));
    let mut restarted = over(busy.kube.clone(), true, testing::config());
    restarted.kube.clear_calls();
    restarted.sync().await;
    assert!(restarted.has_metric("br_sandbox_keeper_pool_desired{pool=\"sbx\"} 9"));
    assert!(restarted.has_metric("br_sandbox_keeper_claims_per_second{pool=\"sbx\"} 0"));
    assert_eq!(restarted.replicas(), Some(14));
    assert!(restarted.replica_writes().is_empty());
    assert!(restarted.deletes().is_empty());
    assert_eq!(restarted.replicaset_deletes(), 0);
    assert!(restarted.creations().is_empty());
    assert_eq!(
        restarted.condition("Accepted"),
        Some(("True".to_owned(), "Valid".to_owned()))
    );
}

#[tokio::test(start_paused = true)]
async fn a_restarted_replica_keeps_the_replicas_for_the_cooldown_and_then_falls_by_one_step() {
    let busy = busy_pool().await;
    let mut restarted = over(busy.kube.clone(), true, testing::config());
    restarted.sync().await;
    restarted.kube.clear_calls();
    restarted.pass(100).await;
    assert_eq!(restarted.replicas(), Some(14));
    assert!(restarted.replica_writes().is_empty());
    restarted.pass(30).await;
    assert_eq!(restarted.replicas(), Some(9));
}

#[tokio::test(start_paused = true)]
async fn a_restarted_replica_judges_every_pod_from_the_cluster_alone() {
    let bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.kube.put_replica_set(owned_replica_set(NS, 3));
    bed.kube.put_pod(old_warm("young", 10));
    bed.kube.put_pod(old_warm("old", 2000));
    let mut restarted = over(bed.kube.clone(), true, testing::config());
    restarted.sync().await;
    assert_eq!(restarted.deletes(), vec!["old".to_owned()]);
    assert!(restarted.kube.pod(NS, "young").is_some());
}
