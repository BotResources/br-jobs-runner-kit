use std::time::Duration;

use br_runner_sandbox::{Error, ExecError, RecordError, manifest_key};
use serde_json::json;

use crate::fakes::{Ids, POD, Script, StdoutModel, rig, rig_with};
use crate::flow_support::{failed_with, noisy_tar_of, record_of};
use crate::part_tar::sample_tar;

const LARGE: usize = 1_500_000;
const STALL: usize = 6 * 64 * 1024;
const END_BLOCKS: usize = 1024;

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_save_that_times_out_with_chunks_queued_stops_the_reader() {
    let rig = rig_with(json!({"save_timeout_seconds": 20}));
    let ids = Ids::new();
    let key = ids.key(&rig);
    let model = StdoutModel::stalling_after(STALL);
    rig.store.delay_each_upload_write(Duration::from_secs(3600));
    rig.cluster
        .push_script(Script::out(noisy_tar_of(LARGE)).through(&model));
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(error, Error::Exec(ExecError::TimedOut { .. })));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Timeout);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
    let writes = rig
        .store
        .log()
        .iter()
        .filter(|line| line.starts_with("upload_write"))
        .count();
    assert_eq!(writes, 1);
    assert_eq!(model.delivered(), STALL);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(model.dropped());
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_an_error_of_the_output_stream_after_some_chunks_is_a_failed_save() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    let model = StdoutModel::failing_after(STALL);
    rig.cluster
        .push_script(Script::out(noisy_tar_of(LARGE)).through(&model));
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(error, Error::Exec(ExecError::Io { .. })));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Exec);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
    assert_eq!(rig.store.aborted().len(), 1);
    assert!(rig.cluster.patches().is_empty());
    assert!(model.dropped());
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_panic_of_the_reader_task_is_a_failed_save_and_not_a_hang() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    let model = StdoutModel::panicking_after(STALL);
    rig.cluster
        .push_script(Script::out(noisy_tar_of(LARGE)).through(&model));
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(error, Error::Exec(ExecError::Io { .. })));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Exec);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
    assert_eq!(rig.store.aborted().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_stream_cut_between_two_entries_or_inside_the_end_blocks_is_a_failed_save()
 {
    let tar = sample_tar();
    let end = tar.len() - END_BLOCKS;
    for cut in [512, 1536, end, end + 100, end + 512, tar.len() - 1] {
        let rig = rig();
        let ids = Ids::new();
        let key = ids.key(&rig);
        rig.cluster.push_script(Script::out(tar[..cut].to_vec()));
        rig.env
            .save(POD, &key, None)
            .await
            .expect_err("a cut stream");
        assert_eq!(
            failed_with(&record_of(&rig, &key)),
            RecordError::Exec,
            "cut {cut}"
        );
        assert!(rig.store.object(&manifest_key(&key)).is_none(), "cut {cut}");
        assert_eq!(rig.store.aborted().len(), 1, "cut {cut}");
    }
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_tar_with_its_end_blocks_is_a_complete_save() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.cluster.push_script(Script::out(sample_tar()));
    rig.env.save(POD, &key, None).await.expect("a save");
    assert!(rig.store.object(&manifest_key(&key)).is_some());
}
