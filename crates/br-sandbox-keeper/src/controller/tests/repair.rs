use serde_json::{Value, json};

use crate::cluster::{FIELD_MANAGER, TEMPLATE_FIELD_MANAGER};

use crate::facts::{Mark, PodFacts};

use super::*;

const HASH_KEY: &str = "br.sandbox/template-hash";

pub(super) fn template_of(bed: &Bed) -> Value {
    serde_json::to_value(
        bed.kube
            .raw_replica_set(NS, RS)
            .and_then(|set| set.spec)
            .and_then(|spec| spec.template)
            .unwrap(),
    )
    .unwrap()
}

fn resized(minimum: u32, generation: i64) -> SandboxPool {
    let mut changed = spec();
    changed.size = PoolSize::new(
        SizeBounds::new("750m", "3"),
        SizeBounds::new("2Gi", "6Gi"),
        SizeBounds::new("2Gi", "9Gi"),
    );
    changed.min_replicas = minimum;
    edited(&changed, generation)
}

fn restore_hash(bed: &Bed, hash: &str) {
    let mut set = bed.kube.raw_replica_set(NS, RS).unwrap();
    set.spec
        .as_mut()
        .and_then(|spec| spec.template.as_mut())
        .unwrap()
        .metadata
        .get_or_insert_with(Default::default)
        .annotations
        .get_or_insert_with(Default::default)
        .insert(HASH_KEY.to_owned(), hash.to_owned());
    bed.kube.put_replica_set(set);
}

pub(super) async fn damaged_as_the_previous_version_left_it(restore: bool) -> (Bed, Value) {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.sync().await;
    bed.kube.put_pool(NS, resized(1, 2));
    bed.settle().await;
    let whole = template_of(&bed);
    let hash = bed.template_hash().unwrap();
    bed.kube
        .rename_field_manager(NS, RS, TEMPLATE_FIELD_MANAGER, FIELD_MANAGER);
    bed.kube
        .apply_as(
            FIELD_MANAGER,
            NS,
            RS,
            &json!({"spec": {"replicas": bed.replicas().unwrap()}}),
        )
        .unwrap();
    assert_ne!(template_of(&bed), whole);
    if restore {
        restore_hash(&bed, &hash);
    }
    assert_eq!(
        restore,
        bed.template_hash().as_deref() == Some(hash.as_str())
    );
    bed.kube.clear_calls();
    (bed, whole)
}

#[tokio::test(start_paused = true)]
async fn a_pool_that_the_keeper_made_gets_one_template_write_and_no_other_at_every_later_pass() {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.sync().await;
    assert_eq!(bed.template_writes(), 1);
    bed.pass(60).await;
    assert_eq!(bed.template_writes(), 1);
    bed.kube.put_pool(NS, resized(5, 2));
    bed.settle().await;
    bed.pass(60).await;
    assert_eq!(bed.template_writes(), 2);
    assert_eq!(bed.replicas(), Some(5));
}

#[tokio::test(start_paused = true)]
async fn a_pool_that_lost_template_fields_with_its_hash_still_current_is_repaired_by_one_write() {
    let (mut bed, whole) = damaged_as_the_previous_version_left_it(true).await;
    bed.pass(60).await;
    assert_eq!(bed.template_writes(), 1);
    assert_eq!(template_of(&bed), whole);
}

#[tokio::test(start_paused = true)]
async fn a_pool_that_lost_template_fields_and_its_hash_is_repaired_by_one_write() {
    let (mut bed, whole) = damaged_as_the_previous_version_left_it(false).await;
    bed.pass(60).await;
    assert_eq!(bed.template_writes(), 1);
    assert_eq!(template_of(&bed), whole);
}

#[tokio::test(start_paused = true)]
async fn a_repaired_pool_changes_its_replicas_and_keeps_the_whole_template() {
    let (mut bed, whole) = damaged_as_the_previous_version_left_it(true).await;
    bed.pass(30).await;
    bed.kube.put_pool(NS, resized(6, 3));
    bed.settle().await;
    bed.pass(30).await;
    assert_eq!(bed.replicas(), Some(6));
    assert_eq!(template_of(&bed), whole);
    assert_eq!(bed.template_writes(), 1);
}

const ROLLOUT_NONE: &str = "br_sandbox_keeper_pods_deleted_total{reason=\"rollout\"} 0";

fn warm_pods_of(hash: &str) -> Vec<PodFacts> {
    [("p0", true), ("p1", true), ("p2", false)]
        .into_iter()
        .map(|(name, ready)| PodFacts {
            ready,
            template_hash: Mark::Value(hash.to_owned()),
            ..old_warm(name, 5)
        })
        .collect()
}

fn put_all(bed: &Bed, pods: Vec<PodFacts>) -> Vec<PodFacts> {
    pods.into_iter().map(|pod| bed.kube.put_pod(pod)).collect()
}

fn assert_pods_untouched(bed: &Bed, pods: &[PodFacts]) {
    assert!(bed.deletes().is_empty());
    assert!(bed.has_metric(ROLLOUT_NONE));
    for pod in pods {
        assert_eq!(bed.kube.pod(NS, &pod.name).as_ref(), Some(pod));
    }
}

#[tokio::test(start_paused = true)]
async fn the_repair_write_on_an_unchanged_hash_replaces_no_warm_pod_and_starts_no_rollout() {
    let (mut bed, whole) = damaged_as_the_previous_version_left_it(true).await;
    let hash = bed.template_hash().unwrap();
    let pods = put_all(&bed, warm_pods_of(&hash));
    bed.pass(120).await;
    assert_eq!(bed.template_writes(), 1);
    assert_eq!(bed.template_hash().as_deref(), Some(hash.as_str()));
    assert_eq!(template_of(&bed), whole);
    assert_pods_untouched(&bed, &pods);
}

#[tokio::test(start_paused = true)]
async fn the_repair_write_with_the_hash_lost_replaces_no_warm_pod_and_starts_no_rollout() {
    let (mut bed, whole) = damaged_as_the_previous_version_left_it(false).await;
    let hash = {
        let mut probe = bare(true);
        probe.kube.put_pool(NS, resized(1, 2));
        probe.sync().await;
        probe.template_hash().unwrap()
    };
    let pods = put_all(&bed, warm_pods_of(&hash));
    bed.pass(120).await;
    assert_eq!(bed.template_writes(), 1);
    assert_eq!(bed.template_hash().as_deref(), Some(hash.as_str()));
    assert_eq!(template_of(&bed), whole);
    assert_pods_untouched(&bed, &pods);
}

#[tokio::test(start_paused = true)]
async fn the_template_write_of_a_replica_set_that_the_keeper_made_replaces_no_warm_pod() {
    let hash = {
        let mut probe = bare(true);
        probe.kube.put_pool(NS, main_pool());
        probe.sync().await;
        probe.template_hash().unwrap()
    };
    let mut bed = bare(true);
    let pods = put_all(&bed, warm_pods_of(&hash));
    bed.kube.put_pool(NS, main_pool());
    bed.sync().await;
    assert_eq!(bed.template_hash().as_deref(), Some(hash.as_str()));
    bed.pass(120).await;
    assert_eq!(bed.template_writes(), 1);
    assert_pods_untouched(&bed, &pods);
}
