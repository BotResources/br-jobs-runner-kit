use br_agentic_harness_contract::{AlreadySaved, PortError};
use br_runner_sandbox::{
    ClusterError, Error, ExecError, FormatError, Manifest, RecordError, RecordState, manifest_key,
};
use serde_json::json;

use crate::fakes::{Ids, POD, Script, rig, rig_with};
use crate::flow_support::{failed_with, record_of};
use crate::part_tar::sample_tar;

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_failing_tar_call_is_a_fault_with_the_record_failed_exec_and_no_manifest()
 {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.cluster.push_script(
        Script {
            stdout: sample_tar(),
            code: Some(1),
            ..Script::default()
        }
        .with_stderr("bsdtar: SECRET-STDERR: cannot stat"),
    );
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(
        error,
        Error::Exec(ExecError::Failed { code: 1, .. })
    ));
    assert!(!error.to_string().contains("SECRET-STDERR"));
    let port = PortError::from(error);
    assert!(!port.is_refusal());
    assert!(!AlreadySaved::is(&port));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Exec);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
    assert_eq!(rig.store.aborted().len(), 1);
    assert!(rig.cluster.patches().is_empty());
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_stream_with_no_status_is_a_fault_and_the_record_failed_exec() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.cluster.push_script(Script {
        stdout: sample_tar(),
        ..Script::default()
    });
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(error, Error::Exec(ExecError::NoStatus { .. })));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Exec);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_tar_that_stops_inside_an_entry_fails_the_save_with_exec() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    let mut cut = sample_tar();
    cut.truncate(700);
    rig.cluster.push_script(Script::out(cut));
    rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Exec);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
}

#[tokio::test(start_paused = true)]
async fn fake_store_a_failing_upload_is_a_fault_with_the_record_failed_store_and_no_manifest() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.store.fail("upload_write", "part-0");
    rig.cluster.push_script(Script::out(sample_tar()));
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(error, Error::Store(_)));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Store);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
    assert_eq!(rig.store.aborted().len(), 1);
    assert!(rig.cluster.patches().is_empty());
}

#[tokio::test(start_paused = true)]
async fn fake_store_a_stored_size_that_is_not_the_written_size_is_a_fault_and_writes_no_manifest() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.store.lie_about_size("part-0", 1);
    rig.cluster.push_script(Script::out(sample_tar()));
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(
        error,
        Error::Format(FormatError::StoredSize { found: 1, .. })
    ));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Store);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
}

#[tokio::test(start_paused = true)]
async fn fake_store_a_failing_manifest_write_is_a_fault_with_the_record_failed_store() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.store.fail("put_if_absent", "manifest.json");
    rig.cluster.push_script(Script::out(sample_tar()));
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(!AlreadySaved::is(&PortError::from(error)));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Store);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_save_that_exceeds_the_save_timeout_is_a_fault_with_the_record_failed_timeout()
 {
    let rig = rig_with(json!({"save_timeout_seconds": 20}));
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.cluster.push_script(Script::hanging());
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(error, Error::Exec(ExecError::TimedOut { .. })));
    assert!(!PortError::from(error).is_refusal());
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Timeout);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_pod_that_is_gone_makes_the_save_a_fault() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.cluster.push_script(Script {
        refuse: Some(ClusterError::Gone(POD.to_owned())),
        ..Script::default()
    });
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(error, Error::Cluster(ClusterError::Gone(_))));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Exec);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_failed_pod_patch_still_gives_the_receipt_and_the_save_exists() {
    let rig = rig();
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.cluster.fail_patches();
    rig.cluster.push_script(Script::out(sample_tar()));
    let outcome = rig.env.save(POD, &key, None).await.expect("the receipt");
    assert!(!outcome.pod_marked);
    assert_eq!(outcome.receipt.turn, ids.turn);
    assert!(rig.store.object(&manifest_key(&key)).is_some());
    assert!(matches!(
        record_of(&rig, &key).state(),
        RecordState::Done { .. }
    ));
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_saves_that_differ_in_one_part_of_the_key_are_separate_saves() {
    let rig = rig();
    let base = Ids::new();
    let variants = [
        Ids {
            thread: crate::common::thread(),
            ..Ids::new()
        },
        Ids {
            agent: crate::common::agent(),
            thread: base.thread,
            ..Ids::new()
        },
        Ids {
            turn: crate::common::turn(),
            thread: base.thread,
            agent: base.agent,
            parent: base.parent,
        },
    ];
    let mut keys = vec![base.key(&rig)];
    keys.extend(variants.iter().map(|ids| ids.key(&rig)));
    for key in &keys {
        rig.cluster.push_script(Script::out(sample_tar()));
        rig.env.save(POD, key, None).await.expect("a save");
    }
    let manifests: Vec<String> = rig
        .store
        .keys()
        .into_iter()
        .filter(|stored| stored.ends_with("manifest.json"))
        .collect();
    assert_eq!(manifests.len(), 4);
    let parts = rig
        .store
        .keys()
        .into_iter()
        .filter(|stored| stored.ends_with("part-0.tar.zst"))
        .count();
    assert_eq!(parts, 4);
    let mut attempts: Vec<_> = keys
        .iter()
        .map(|key| {
            Manifest::parse(&rig.store.object(&manifest_key(key)).expect("manifest"))
                .expect("valid")
                .attempt()
        })
        .collect();
    attempts.sort_by_key(|attempt| attempt.as_uuid());
    attempts.dedup();
    assert_eq!(attempts.len(), 4);
}
