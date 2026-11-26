use br_agentic_harness_contract::FreshReason;
use br_runner_sandbox::{
    AttemptId, Manifest, PartEncoder, Record, StartPlan, StoreView, Tombstone, manifest_key,
    plan_start, record_key, tombstone_key,
};

use crate::fakes::{Ids, Rig, rig, start_time};

fn manifest_for(rig: &Rig, ids: &Ids) -> Manifest {
    let mut encoder = PartEncoder::new(3).expect("an encoder");
    encoder.push(&crate::part_tar::sample_tar()).expect("push");
    let (_, report) = encoder.finish().expect("finish");
    Manifest::new(
        ids.parent_key(rig),
        None,
        AttemptId::mint(),
        start_time(),
        vec!["/work".to_owned()],
        report,
    )
}

fn gets(rig: &Rig) -> Vec<String> {
    rig.store
        .log()
        .into_iter()
        .filter(|line| line.starts_with("get "))
        .collect()
}

#[tokio::test(start_paused = true)]
async fn fake_store_the_manifest_is_read_first_and_the_other_two_objects_are_not_read() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.parent_key(&rig);
    let manifest = manifest_for(&rig, &ids);
    rig.store
        .insert(&manifest_key(&key), manifest.to_vec().expect("bytes"));
    rig.store.insert(
        &tombstone_key(&key),
        Tombstone::new(ids.parent, start_time(), start_time())
            .to_vec()
            .expect("bytes"),
    );
    let view = rig.env.read_store_view(&key).await;
    assert_eq!(view, StoreView::Manifest(Box::new(manifest)));
    assert_eq!(gets(&rig).len(), 1);
}

#[tokio::test(start_paused = true)]
async fn fake_store_the_tombstone_is_read_only_when_the_manifest_is_absent_and_the_record_not_then()
{
    let rig = rig();
    let ids = Ids::new();
    let key = ids.parent_key(&rig);
    let tombstone = Tombstone::new(ids.parent, start_time(), start_time());
    rig.store
        .insert(&tombstone_key(&key), tombstone.to_vec().expect("bytes"));
    let view = rig.env.read_store_view(&key).await;
    assert_eq!(view, StoreView::Tombstone(tombstone));
    let reads = gets(&rig);
    assert_eq!(reads.len(), 2);
    assert!(reads[0].contains("manifest.json"));
    assert!(reads[1].contains("tombstones/"));
}

#[tokio::test(start_paused = true)]
async fn fake_store_the_record_is_read_last_and_at_most_three_reads_are_made() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.parent_key(&rig);
    let record = Record::started(AttemptId::mint(), start_time());
    rig.store
        .insert(&record_key(&key), record.to_vec().expect("bytes"));
    assert_eq!(
        rig.env.read_store_view(&key).await,
        StoreView::Record(record)
    );
    assert_eq!(gets(&rig).len(), 3);
    let empty = crate::fakes::rig();
    assert_eq!(
        empty.env.read_store_view(&ids.parent_key(&empty)).await,
        StoreView::Nothing
    );
    assert_eq!(gets(&empty).len(), 3);
}

#[tokio::test(start_paused = true)]
async fn fake_store_a_read_that_fails_is_a_store_that_is_unavailable_not_an_error() {
    for failing in ["manifest.json", "tombstones/", "records/"] {
        let rig = rig();
        let ids = Ids::new();
        rig.store.fail("get", failing);
        let view = rig.env.read_store_view(&ids.parent_key(&rig)).await;
        assert_eq!(view, StoreView::Unavailable, "{failing}");
        assert_eq!(
            plan_start(view, ids.parent, start_time(), rig.config.save_ttl()),
            StartPlan::Fresh(FreshReason::StoreUnavailable)
        );
    }
}

#[tokio::test(start_paused = true)]
async fn fake_store_an_object_that_is_not_valid_or_is_of_another_key_is_never_served_for_a_start() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.parent_key(&rig);
    rig.store.insert(&manifest_key(&key), "not json");
    assert_eq!(rig.env.read_store_view(&key).await, StoreView::Unavailable);
    let other = Ids::new();
    let foreign = manifest_for(&rig, &other);
    rig.store
        .insert(&manifest_key(&key), foreign.to_vec().expect("bytes"));
    assert_eq!(rig.env.read_store_view(&key).await, StoreView::Unavailable);
}
