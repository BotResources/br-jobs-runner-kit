use super::repair::{damaged_as_the_previous_version_left_it, template_of};
use super::rollout::{current, stale, with_step};
use crate::facts::PodFacts;

use super::*;

const ROLLOUT_FOUR: &str = "br_sandbox_keeper_pods_deleted_total{reason=\"rollout\"} 4";
const STALE_IN_ORDER: [&str; 4] = ["w1", "w0", "w2", "w3"];
const ROLLOUT_NONE: &str = "br_sandbox_keeper_pods_deleted_total{reason=\"rollout\"} 0";

fn put_stale_and_current(bed: &Bed) -> Vec<PodFacts> {
    for (name, age, ready) in [
        ("w0", 40, true),
        ("w1", 30, false),
        ("w2", 20, true),
        ("w3", 10, true),
    ] {
        bed.kube.put_pod(stale(name, age, ready));
    }
    ["r0", "r1"]
        .into_iter()
        .map(|name| bed.kube.put_pod(current(bed, name, true)))
        .collect()
}

fn assert_current_pods_stay(bed: &Bed, put: &[PodFacts]) {
    for pod in put {
        assert_eq!(bed.kube.pod(NS, &pod.name).as_ref(), Some(pod));
    }
}

#[tokio::test(start_paused = true)]
async fn the_repair_write_with_stale_warm_pods_present_leaves_the_rollout_to_its_own_rule() {
    let (mut bed, whole) = damaged_as_the_previous_version_left_it(true).await;
    with_step(&mut bed, 2);
    let hash = bed.template_hash().unwrap();
    let put = put_stale_and_current(&bed);
    bed.sync().await;
    assert_eq!(bed.template_writes(), 1);
    assert_eq!(bed.template_hash().as_deref(), Some(hash.as_str()));
    assert_eq!(template_of(&bed), whole);
    assert_eq!(bed.deletes(), STALE_IN_ORDER);
    assert!(bed.has_metric(ROLLOUT_FOUR));
    for name in STALE_IN_ORDER {
        assert!(bed.kube.pod(NS, name).is_none(), "{name}");
    }
    assert_current_pods_stay(&bed, &put);
}

#[tokio::test(start_paused = true)]
async fn a_failed_repair_write_is_counted_pauses_the_rollout_and_is_tried_again() {
    let (logs, _guard) = crate::testing::logs::capture();
    let (mut bed, whole) = damaged_as_the_previous_version_left_it(true).await;
    with_step(&mut bed, 2);
    let hash = bed.template_hash().unwrap();
    let put = put_stale_and_current(&bed);
    let errors = bed.errors();
    bed.kube.fail("apply_template");
    bed.sync().await;
    bed.pass(10).await;
    assert!(bed.errors() > errors);
    let logged = logs
        .text()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|line| {
            line["level"] == "WARN"
                && line["fields"]["message"] == "the call failed"
                && line["fields"]["call"] == "apply_template"
        });
    assert!(logged);
    let writes: Vec<Call> = bed
        .pool_writes()
        .into_iter()
        .filter(|call| !matches!(call, Call::ApplyTemplate { .. } | Call::Condition { .. }))
        .collect();
    assert!(writes.is_empty(), "{writes:?}");
    assert!(bed.deletes().is_empty());
    assert!(bed.has_metric(ROLLOUT_NONE));
    assert_ne!(template_of(&bed), whole);
    bed.kube.heal("apply_template");
    bed.kube.clear_calls();
    bed.pass(10).await;
    assert_eq!(bed.template_writes(), 1);
    assert_eq!(template_of(&bed), whole);
    assert_eq!(bed.template_hash().as_deref(), Some(hash.as_str()));
    assert_eq!(bed.deletes(), STALE_IN_ORDER);
    assert!(bed.has_metric(ROLLOUT_FOUR));
    assert_current_pods_stay(&bed, &put);
    bed.kube.clear_calls();
    bed.pass(10).await;
    assert_eq!(bed.template_writes(), 0);
}
