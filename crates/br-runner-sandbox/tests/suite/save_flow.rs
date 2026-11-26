use br_agentic_harness_contract::{AlreadySaved, PortError};
use br_runner_sandbox::{
    Error, Manifest, PartDecoder, PodKeys, RecordState, argv, manifest_key, part_key, saved_patch,
};
use serde_json::json;

use crate::fakes::{Ids, POD, Script, rig, start_time};
use crate::flow_support::{noisy_tar, position, record_of};
use crate::part_tar::sample_tar;

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_save_runs_its_steps_in_order_and_gives_the_receipt() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    let tar = sample_tar();
    rig.cluster.push_script(Script::out(tar.clone()));
    let outcome = rig
        .env
        .save(POD, &key, Some(ids.parent))
        .await
        .expect("a save");
    let log = rig.store.log();
    let order = [
        position(&log, "size saves/acme/"),
        position(&log, "put records/"),
        position(&log, "upload_start"),
        position(&log, "upload_finish"),
        position(&log, "put_if_absent"),
    ];
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{log:?}");
    let part_size = log
        .iter()
        .rposition(|line| line.starts_with("size ") && line.contains("part-0"))
        .expect("the size check of the part");
    assert!(position(&log, "upload_finish") < part_size);
    assert!(part_size < position(&log, "put_if_absent"));
    assert_eq!(outcome.receipt.turn, ids.turn);
    assert_eq!(outcome.receipt.bytes, tar.len() as u64);
    assert_eq!(outcome.receipt.made_at, start_time().to_system_time());
    assert!(outcome.pod_marked);
    let execs = rig.cluster.execs();
    assert_eq!(execs.len(), 1);
    assert_eq!(
        execs[0].request.command,
        argv::tar_create(rig.config.tools_path(), rig.config.save_scope_paths())
    );
    assert_eq!(execs[0].request.container, "sandbox");
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_the_manifest_the_part_and_the_record_describe_the_same_save() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    let tar = sample_tar();
    rig.cluster.push_script(Script::out(tar.clone()));
    rig.env
        .save(POD, &key, Some(ids.parent))
        .await
        .expect("a save");
    let manifest = Manifest::parse_for(
        &rig.store.object(&manifest_key(&key)).expect("a manifest"),
        &key,
    )
    .expect("a valid manifest");
    assert_eq!(manifest.parent(), Some(ids.parent));
    assert_eq!(manifest.scope(), ["/work", "/home/agent"]);
    assert_eq!(manifest.entries(), 5);
    assert_eq!(manifest.part().key, part_key(&key, manifest.attempt()));
    assert_eq!(manifest.part().bytes_raw, tar.len() as u64);
    let stored = rig.store.object(&manifest.part().key).expect("a part");
    assert_eq!(stored.len() as u64, manifest.part().bytes_stored);
    let mut decoder = PartDecoder::new(manifest.part()).expect("a decoder");
    let mut raw = decoder.push(&stored).expect("raw bytes");
    raw.extend(decoder.finish().expect("a complete part"));
    assert_eq!(raw, tar);
    let record = record_of(&rig, &key);
    assert_eq!(record.attempt(), manifest.attempt());
    assert!(matches!(record.state(), RecordState::Done { .. }));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_the_pod_gets_the_saved_marks_and_no_idle_mark() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.cluster.push_script(Script::out(sample_tar()));
    let outcome = rig.env.save(POD, &key, None).await.expect("a save");
    let patches = rig.cluster.patches();
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].0, POD);
    let expected = saved_patch(
        &PodKeys::default(),
        ids.turn,
        outcome.receipt.bytes,
        start_time(),
    );
    assert_eq!(patches[0].1, expected);
    let annotations = &patches[0].1["metadata"]["annotations"];
    assert_eq!(annotations["br.sandbox/clean"], json!("true"));
    assert_eq!(
        annotations["br.sandbox/state-turn"],
        json!(ids.turn.to_string())
    );
    assert_eq!(
        annotations["br.sandbox/state-bytes"],
        json!(outcome.receipt.bytes.to_string())
    );
    assert!(annotations.get("br.sandbox/idle-since").is_none());
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_save_streams_a_large_tar_through_several_upload_writes() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    let tar = noisy_tar();
    rig.cluster.push_script(Script::out(tar.clone()));
    rig.env.save(POD, &key, None).await.expect("a save");
    let writes = rig
        .store
        .log()
        .iter()
        .filter(|line| line.starts_with("upload_write"))
        .count();
    assert!(writes >= 2, "{writes} writes");
    let manifest = Manifest::parse_for(
        &rig.store.object(&manifest_key(&key)).expect("a manifest"),
        &key,
    )
    .expect("a manifest");
    assert_eq!(manifest.part().bytes_raw, tar.len() as u64);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_save_of_a_turn_that_has_a_manifest_is_refused_and_writes_nothing()
{
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.store.insert(&manifest_key(&key), "the first save");
    let error = rig.env.save(POD, &key, None).await.expect_err("refused");
    assert!(matches!(error, Error::AlreadySaved));
    let port = PortError::from(error);
    assert!(AlreadySaved::is(&port));
    assert!(rig.store.log().iter().all(|line| line.starts_with("size ")));
    assert!(rig.cluster.execs().is_empty());
    assert!(rig.cluster.patches().is_empty());
    assert_eq!(
        rig.store.object(&manifest_key(&key)).expect("kept"),
        "the first save"
    );
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_second_save_of_one_key_is_refused_and_the_first_stays() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.cluster.push_script(Script::out(sample_tar()));
    rig.env.save(POD, &key, None).await.expect("a save");
    let first = rig.store.object(&manifest_key(&key)).expect("a manifest");
    rig.cluster.push_script(Script::out(sample_tar()));
    let error = rig.env.save(POD, &key, None).await.expect_err("refused");
    assert!(AlreadySaved::is(&PortError::from(error)));
    assert_eq!(rig.store.object(&manifest_key(&key)), Some(first));
    assert_eq!(rig.cluster.execs().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn fake_store_a_lost_race_on_the_manifest_is_the_same_refusal_and_leaves_the_part_for_the_keeper()
 {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.store
        .rival_before_create(&manifest_key(&key), "the rival");
    rig.cluster.push_script(Script::out(sample_tar()));
    let error = rig.env.save(POD, &key, None).await.expect_err("refused");
    assert!(AlreadySaved::is(&PortError::from(error)));
    assert_eq!(
        rig.store
            .object(&manifest_key(&key))
            .expect("the rival stays"),
        "the rival"
    );
    assert!(
        rig.store
            .keys()
            .iter()
            .any(|stored| stored.ends_with("part-0.tar.zst"))
    );
    assert!(matches!(
        record_of(&rig, &key).state(),
        RecordState::Started
    ));
    assert!(rig.cluster.patches().is_empty());
}
