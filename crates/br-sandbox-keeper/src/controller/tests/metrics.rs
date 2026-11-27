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
        kept_idle("c0", "sbx"),
        kept_idle("c1", "sbx"),
        kept_idle("c2", "unconfigured"),
    ] {
        bed.kube.put_pod(pod);
        bed.settle().await;
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
        ..kept_idle("d0", "sbx")
    };
    let unreadable = PodFacts {
        clean: Mark::Unreadable,
        ..kept_idle("u0", "sbx")
    };
    let absent = PodFacts {
        clean: Mark::Absent,
        ..kept_idle("a0", "sbx")
    };
    for pod in [
        dirty,
        unreadable,
        absent,
        old_warm("w0", 10),
        kept_idle("c0", "sbx"),
    ] {
        bed.kube.put_pod(pod);
        bed.settle().await;
    }
    assert_eq!(
        series(&bed, "idle_pods"),
        vec!["br_sandbox_keeper_idle_pods 1"]
    );
}

#[tokio::test(start_paused = true)]
async fn idle_pods_follows_the_deletes_of_the_watch() {
    let mut bed = bed(true);
    bed.kube.put_pod(kept_idle("c0", "sbx"));
    bed.settle().await;
    assert_eq!(
        series(&bed, "idle_pods"),
        vec!["br_sandbox_keeper_idle_pods 1"]
    );
    bed.kube.remove_pod(NS, "c0");
    bed.settle().await;
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
        bed.kube.put_pod(pod);
        bed.settle().await;
    }
    assert_eq!(
        series(&bed, "pool_warm_ready"),
        vec!["br_sandbox_keeper_pool_warm_ready{pool=\"sbx\"} 1"]
    );
}

#[tokio::test(start_paused = true)]
async fn pool_desired_and_claims_per_second_carry_the_label_pool_of_each_configured_pool() {
    let mut bed = bed(true);
    bed.sync().await;
    assert_eq!(
        series(&bed, "pool_desired"),
        vec!["br_sandbox_keeper_pool_desired{pool=\"sbx\"} 2"]
    );
    assert_eq!(
        series(&bed, "claims_per_second"),
        vec!["br_sandbox_keeper_claims_per_second{pool=\"sbx\"} 0"]
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_pod_watch_is_logged_at_error_level_with_its_reason_and_counted() {
    let (logs, _guard) = crate::testing::logs::capture();
    let mut bed = bed(true);
    bed.sync().await;
    let before = bed.errors();
    let reasons = [
        "the pod list failed: the server is down",
        "the pod watch broke: connection reset",
    ];
    for reason in reasons {
        bed.kube.break_watch("pods", NS, reason);
        bed.pass(30).await;
    }
    assert_eq!(bed.errors(), before + 2);
    let lines: Vec<serde_json::Value> = logs
        .text()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|line| line["level"] == "ERROR")
        .filter(|line| line["fields"]["message"] == "the pod watch of a namespace failed")
        .collect();
    assert_eq!(lines.len(), 2);
    for (line, reason) in lines.iter().zip(reasons) {
        assert_eq!(line["fields"]["reason"], reason);
        assert_eq!(line["fields"]["namespace"], NS);
    }
}
