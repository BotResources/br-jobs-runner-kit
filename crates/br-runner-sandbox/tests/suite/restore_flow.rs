use std::time::Duration;

use br_runner_sandbox::{
    ClusterError, PartReport, RestoreVerdict, Sha256Hex, argv, restore_failed,
};
use serde_json::json;

use crate::fakes::{POD, Script, rig, rig_with};
use crate::flow_support::{position, stage, stage_with};
use crate::part_tar::sample_tar;

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_restore_streams_the_decoded_tar_into_bsdtar_and_closes_its_input()
{
    let rig = rig();
    let staged = stage(&rig);
    rig.store.stream_in_chunks_of(7);
    rig.cluster.push_script(Script::like_bsdtar_extract());
    rig.store.clear_log();
    let verdict = rig
        .env
        .restore(POD, &staged.manifest, staged.ids.parent)
        .await;
    assert_eq!(verdict, RestoreVerdict::Restored);
    let execs = rig.cluster.execs();
    assert_eq!(execs.len(), 1);
    assert_eq!(
        execs[0].request.command,
        argv::tar_restore_scoped(rig.config.tools_path(), rig.config.save_scope_paths())
    );
    assert_eq!(execs[0].request.container, "sandbox");
    assert_eq!(execs[0].stdin(), staged.tar);
    assert!(execs[0].stdin_closed());
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_the_manifest_is_read_again_after_the_parts_are_streamed() {
    let rig = rig();
    let staged = stage(&rig);
    rig.cluster.push_script(Script::like_bsdtar_extract());
    rig.store.clear_log();
    rig.env
        .restore(POD, &staged.manifest, staged.ids.parent)
        .await;
    let log = rig.store.log();
    assert!(
        position(&log, "get_stream") < position(&log, "get saves/"),
        "{log:?}"
    );
    assert_eq!(
        log.iter()
            .filter(|line| line.starts_with("get saves/"))
            .count(),
        1
    );
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_missing_part_is_a_restore_that_failed_and_no_exec_starts() {
    let rig = rig();
    let staged = stage(&rig);
    rig.store.remove(&staged.manifest.part().key);
    let verdict = rig
        .env
        .restore(POD, &staged.manifest, staged.ids.parent)
        .await;
    assert_eq!(
        verdict,
        RestoreVerdict::Fresh(restore_failed(staged.ids.parent))
    );
    assert!(rig.cluster.execs().is_empty());
}

#[tokio::test(start_paused = true)]
async fn fake_store_a_store_that_fails_on_the_part_is_a_restore_that_failed() {
    let rig = rig();
    let staged = stage(&rig);
    rig.store.fail("get_stream", "part-0");
    rig.cluster.push_script(Script::like_bsdtar_extract());
    let verdict = rig
        .env
        .restore(POD, &staged.manifest, staged.ids.parent)
        .await;
    assert_eq!(
        verdict,
        RestoreVerdict::Fresh(restore_failed(staged.ids.parent))
    );
}

type Change = fn(&mut PartReport);

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_part_with_a_wrong_size_or_hash_is_a_restore_that_failed() {
    let changes: Vec<(&str, Change)> = vec![
        ("raw hash", |report| {
            report.sha256_raw = Sha256Hex::of(b"other")
        }),
        ("stored hash", |report| {
            report.sha256_stored = Sha256Hex::of(b"other")
        }),
        ("stored size longer than the part", |report| {
            report.bytes_stored += 1
        }),
        ("stored size shorter than the part", |report| {
            report.bytes_stored -= 1
        }),
        ("raw size longer than the tar", |report| {
            report.bytes_raw += 1
        }),
    ];
    for (name, change) in changes {
        let rig = rig();
        let staged = stage_with(&rig, sample_tar(), change);
        rig.cluster.push_script(Script::like_bsdtar_extract());
        let verdict = rig
            .env
            .restore(POD, &staged.manifest, staged.ids.parent)
            .await;
        assert_eq!(
            verdict,
            RestoreVerdict::Fresh(restore_failed(staged.ids.parent)),
            "{name}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_part_that_is_not_a_zstd_stream_is_a_restore_that_failed() {
    let rig = rig();
    let staged = stage(&rig);
    let stored = rig
        .store
        .object(&staged.manifest.part().key)
        .expect("a part");
    let mut damaged = stored.to_vec();
    for byte in damaged.iter_mut().skip(8) {
        *byte ^= 0xA5;
    }
    rig.store.insert(&staged.manifest.part().key, damaged);
    rig.cluster.push_script(Script::like_bsdtar_extract());
    let verdict = rig
        .env
        .restore(POD, &staged.manifest, staged.ids.parent)
        .await;
    assert_eq!(
        verdict,
        RestoreVerdict::Fresh(restore_failed(staged.ids.parent))
    );
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_an_extract_that_fails_or_ends_with_no_status_is_a_restore_that_failed()
 {
    for script in [
        Script {
            code: Some(1),
            wait_for_stdin_close: true,
            ..Script::default()
        }
        .with_stderr("bsdtar: SECRET-STDERR"),
        Script {
            wait_for_stdin_close: true,
            ..Script::default()
        },
    ] {
        let rig = rig();
        let staged = stage(&rig);
        rig.cluster.push_script(script);
        let verdict = rig
            .env
            .restore(POD, &staged.manifest, staged.ids.parent)
            .await;
        assert_eq!(
            verdict,
            RestoreVerdict::Fresh(restore_failed(staged.ids.parent))
        );
    }
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_restore_that_never_ends_stops_at_the_restore_timeout() {
    let rig = rig_with(json!({"restore_timeout_seconds": 30}));
    let staged = stage(&rig);
    rig.cluster.push_script(Script::hanging());
    let started = tokio::time::Instant::now();
    let verdict = rig
        .env
        .restore(POD, &staged.manifest, staged.ids.parent)
        .await;
    assert_eq!(
        verdict,
        RestoreVerdict::Fresh(restore_failed(staged.ids.parent))
    );
    assert_eq!(started.elapsed(), Duration::from_secs(30));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_pod_that_is_gone_during_a_restore_is_a_restore_that_failed_not_an_error()
 {
    let rig = rig();
    let staged = stage(&rig);
    rig.cluster.push_script(Script {
        refuse: Some(ClusterError::Gone(POD.to_owned())),
        ..Script::default()
    });
    let verdict = rig
        .env
        .restore(POD, &staged.manifest, staged.ids.parent)
        .await;
    assert_eq!(
        verdict,
        RestoreVerdict::Fresh(restore_failed(staged.ids.parent))
    );
}
