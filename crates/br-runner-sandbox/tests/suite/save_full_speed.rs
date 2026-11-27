use std::time::Duration;

use br_runner_sandbox::{Error, Manifest, PartDecoder, RecordError, manifest_key};

use crate::fakes::{Ids, POD, Rig, Script, StdoutModel, rig};
use crate::flow_support::{failed_with, noisy_tar_of, record_of};

const LARGE: usize = 1_500_000;
const STALL: usize = 6 * 64 * 1024;

fn manifest_of(rig: &Rig, ids: &Ids) -> Manifest {
    let key = ids.key(rig);
    Manifest::parse_for(
        &rig.store.object(&manifest_key(&key)).expect("a manifest"),
        &key,
    )
    .expect("a valid manifest")
}

fn decoded(rig: &Rig, manifest: &Manifest) -> Vec<u8> {
    let stored = rig.store.object(&manifest.part().key).expect("a part");
    let mut decoder = PartDecoder::new(manifest.part()).expect("a decoder");
    let mut raw = decoder.push(&stored).expect("raw bytes");
    raw.extend(decoder.finish().expect("a complete part"));
    raw
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_slow_upload_does_not_make_the_exec_reader_lose_the_tail_of_the_output()
 {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    let tar = noisy_tar_of(LARGE);
    let model = StdoutModel::lossy_after(Duration::from_millis(50));
    rig.store
        .delay_each_upload_write(Duration::from_millis(200));
    rig.cluster
        .push_script(Script::out(tar.clone()).through(&model));
    let outcome = rig
        .env
        .save(POD, &key, None)
        .await
        .expect("a complete save");
    assert_eq!(model.delivered(), tar.len());
    assert_eq!(outcome.receipt.bytes, tar.len() as u64);
    let manifest = manifest_of(&rig, &ids);
    assert_eq!(manifest.part().bytes_raw, tar.len() as u64);
    assert_eq!(decoded(&rig, &manifest), tar);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_stream_that_the_server_cuts_inside_an_entry_is_still_a_failed_save()
 {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    let tar = noisy_tar_of(LARGE);
    let model = StdoutModel::cut_after(700_001);
    rig.cluster
        .push_script(Script::out(tar.clone()).through(&model));
    rig.env
        .save(POD, &key, None)
        .await
        .expect_err("a cut stream");
    assert!(model.delivered() < tar.len());
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Exec);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
    assert_eq!(rig.store.aborted().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_failing_upload_ends_the_save_and_the_read_of_the_output() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    let tar = noisy_tar_of(LARGE);
    let model = StdoutModel::stalling_after(STALL);
    rig.store.fail("upload_write", "part-0");
    rig.cluster
        .push_script(Script::out(tar.clone()).through(&model));
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(error, Error::Store(_)));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Store);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
    assert_eq!(rig.store.aborted().len(), 1);
    assert!(rig.cluster.patches().is_empty());
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(model.dropped());
    let read_at_the_fault = model.delivered();
    assert!(read_at_the_fault <= STALL, "{read_at_the_fault} bytes");
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(model.delivered(), read_at_the_fault);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_the_part_of_a_slow_save_has_the_bytes_the_hashes_and_the_counts_of_the_fast_save()
 {
    let tar = noisy_tar_of(LARGE);
    let fast = rig();
    let fast_ids = Ids::new();
    fast.cluster.push_script(Script::out(tar.clone()));
    fast.env
        .save(POD, &fast_ids.key(&fast), None)
        .await
        .expect("the fast save");
    let slow = rig();
    let slow_ids = Ids::new();
    let model = StdoutModel::yielding();
    slow.store
        .delay_each_upload_write(Duration::from_millis(20));
    slow.cluster
        .push_script(Script::out(tar.clone()).through(&model));
    slow.env
        .save(POD, &slow_ids.key(&slow), None)
        .await
        .expect("the slow save");
    let fast_manifest = manifest_of(&fast, &fast_ids);
    let slow_manifest = manifest_of(&slow, &slow_ids);
    let (fast_part, slow_part) = (fast_manifest.part(), slow_manifest.part());
    assert_eq!(slow_part.bytes_raw, fast_part.bytes_raw);
    assert_eq!(slow_part.bytes_stored, fast_part.bytes_stored);
    assert_eq!(slow_part.sha256_raw, fast_part.sha256_raw);
    assert_eq!(slow_part.sha256_stored, fast_part.sha256_stored);
    assert_eq!(slow_manifest.entries(), fast_manifest.entries());
    assert_eq!(
        slow.store.object(&slow_part.key),
        fast.store.object(&fast_part.key)
    );
    assert_eq!(decoded(&slow, &slow_manifest), tar);
}
