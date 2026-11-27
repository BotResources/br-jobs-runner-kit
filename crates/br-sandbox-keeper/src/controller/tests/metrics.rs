use crate::facts::{Mark, PodFacts, PoolState, fixtures};

use super::*;

fn series(bed: &Bed, name: &str) -> Vec<String> {
    bed.metrics_text()
        .lines()
        .filter(|line| line.starts_with(&format!("br_sandbox_keeper_{name}")))
        .map(str::to_owned)
        .collect()
}

fn kept_idle(name: &str, pool: &str) -> PodFacts {
    PodFacts {
        pool_id: Some(pool.to_owned()),
        idle_since: Mark::Value(at(START - 10)),
        state_bytes: Mark::Value(20_000_000),
        ..fixtures::claimed(name, true, START - 500)
    }
}

#[tokio::test(start_paused = true)]
async fn idle_pods_has_no_label_and_counts_every_claimed_clean_pod_of_a_served_namespace() {
    let mut bed = bed(true);
    for pod in [
        kept_idle("c0", "p"),
        kept_idle("c1", "p"),
        kept_idle("c2", "unconfigured"),
    ] {
        let pod = bed.kube.put_pod(pod);
        bed.applied(pod).await;
    }
    assert_eq!(
        series(&bed, "idle_pods"),
        vec!["br_sandbox_keeper_idle_pods 3"]
    );
}

#[tokio::test(start_paused = true)]
async fn idle_pods_leaves_out_dirty_unreadable_and_warm_pods() {
    let mut bed = bed(true);
    let dirty = PodFacts {
        clean: Mark::Value(false),
        ..kept_idle("d0", "p")
    };
    let unreadable = PodFacts {
        clean: Mark::Unreadable,
        ..kept_idle("u0", "p")
    };
    let absent = PodFacts {
        clean: Mark::Absent,
        ..kept_idle("a0", "p")
    };
    for pod in [
        dirty,
        unreadable,
        absent,
        old_warm("w0", 10),
        kept_idle("c0", "p"),
    ] {
        let pod = bed.kube.put_pod(pod);
        bed.applied(pod).await;
    }
    assert_eq!(
        series(&bed, "idle_pods"),
        vec!["br_sandbox_keeper_idle_pods 1"]
    );
}

#[tokio::test(start_paused = true)]
async fn idle_pods_follows_the_deletes_of_the_watch() {
    let mut bed = bed(true);
    let pod = bed.kube.put_pod(kept_idle("c0", "p"));
    bed.applied(pod).await;
    assert_eq!(
        series(&bed, "idle_pods"),
        vec!["br_sandbox_keeper_idle_pods 1"]
    );
    bed.engine
        .handle(
            NS,
            PodEvent::Deleted {
                name: "c0".to_owned(),
            },
        )
        .await;
    assert_eq!(
        series(&bed, "idle_pods"),
        vec!["br_sandbox_keeper_idle_pods 0"]
    );
}

#[tokio::test(start_paused = true)]
async fn pool_warm_ready_counts_the_ready_warm_pods_of_the_pool_with_the_label_pool() {
    let mut bed = bed(true);
    bed.sync().await;
    let not_ready = PodFacts {
        ready: false,
        ..old_warm("w1", 10)
    };
    let other_pool = PodFacts {
        pool_id: Some("other".to_owned()),
        ..old_warm("w2", 10)
    };
    let claimed = PodFacts {
        ready: true,
        pool_state: PoolState::Claimed,
        ..old_warm("w3", 10)
    };
    for pod in [old_warm("w0", 10), not_ready, other_pool, claimed] {
        let pod = bed.kube.put_pod(pod);
        bed.applied(pod).await;
    }
    assert_eq!(
        series(&bed, "pool_warm_ready"),
        vec!["br_sandbox_keeper_pool_warm_ready{pool=\"p\"} 1"]
    );
}

#[tokio::test(start_paused = true)]
async fn pool_desired_and_claims_per_second_carry_the_label_pool_of_each_configured_pool() {
    let mut bed = bed(true);
    bed.sync().await;
    assert_eq!(
        series(&bed, "pool_desired"),
        vec!["br_sandbox_keeper_pool_desired{pool=\"p\"} 2"]
    );
    assert_eq!(
        series(&bed, "claims_per_second"),
        vec!["br_sandbox_keeper_claims_per_second{pool=\"p\"} 0"]
    );
}
