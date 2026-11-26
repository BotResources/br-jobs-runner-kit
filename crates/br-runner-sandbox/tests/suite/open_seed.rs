use br_agentic_harness_contract::{FilePath, SandboxPort, Seed, SeedEntry};
use br_runner_sandbox::{PodKeys, argv, seed_hash_text};
use serde_json::json;

use crate::fakes::{Lineage, Script, boxes, boxes_with, fresh_request, request, warm_pod};

#[tokio::test(start_paused = true)]
async fn seed_the_seed_is_written_through_the_seed_container_and_its_hash_is_set_on_the_pod() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes
        .cluster
        .scripts
        .push_script(Script::like_bsdtar_extract());
    let entry = SeedEntry::file(
        FilePath::new("skills/a.md").expect("a path"),
        0o644,
        b"hello".to_vec(),
    );
    let seed = Seed::new(vec![entry]).expect("a seed");
    let lineage = Lineage::new();

    let opened = boxes
        .sandbox
        .open(&request(lineage.first(), seed.clone()))
        .await
        .expect("a box");

    let execs = boxes.cluster.scripts.execs();
    assert_eq!(execs.len(), 1);
    assert_eq!(execs[0].request.container, "seed");
    assert_eq!(
        execs[0].request.command,
        argv::tar_seed(boxes.config.seed_tools_path(), boxes.config.seed_root())
    );
    assert!(execs[0].stdin_closed());
    let pod = boxes.cluster.pod(opened.box_id.as_str()).expect("a pod");
    assert_eq!(
        pod.annotations.get(&PodKeys::default().seed_hash()),
        Some(&seed_hash_text(&seed))
    );
}

#[tokio::test(start_paused = true)]
async fn seed_an_empty_seed_writes_no_file_and_still_sets_the_hash() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let lineage = Lineage::new();

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.first()))
        .await
        .expect("a box");

    assert!(boxes.cluster.scripts.execs().is_empty());
    let pod = boxes.cluster.pod(opened.box_id.as_str()).expect("a pod");
    assert_eq!(
        pod.annotations.get(&PodKeys::default().seed_hash()),
        Some(&seed_hash_text(&Seed::empty()))
    );
}

#[tokio::test(start_paused = true)]
async fn seed_when_the_seed_exec_fails_the_pod_is_deleted_and_open_is_a_port_fault() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.scripts.push_script(Script::exit(2));
    let entry = SeedEntry::file(FilePath::new("a.md").expect("a path"), 0o644, b"x".to_vec());
    let lineage = Lineage::new();
    let open = request(lineage.first(), Seed::new(vec![entry]).expect("a seed"));

    let error = boxes.sandbox.open(&open).await.expect_err("a fault");

    assert!(!error.is_refusal());
    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn seed_when_the_seed_exec_ends_with_no_status_the_pod_is_deleted_and_open_is_a_port_fault() {
    let boxes = boxes_with(json!({}));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.scripts.push_script(Script::no_status());
    let entry = SeedEntry::file(FilePath::new("a.md").expect("a path"), 0o644, b"x".to_vec());
    let lineage = Lineage::new();
    let open = request(lineage.first(), Seed::new(vec![entry]).expect("a seed"));

    let error = boxes.sandbox.open(&open).await.expect_err("a fault");

    assert!(!error.is_refusal());
    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn seed_when_the_seed_exec_exceeds_the_exec_timeout_the_pod_is_deleted_and_open_is_a_port_fault()
 {
    let boxes = boxes_with(json!({"exec_timeout_seconds": 5}));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.scripts.push_script(Script::hanging());
    let entry = SeedEntry::file(FilePath::new("a.md").expect("a path"), 0o644, b"x".to_vec());
    let lineage = Lineage::new();
    let open = request(lineage.first(), Seed::new(vec![entry]).expect("a seed"));

    let error = boxes.sandbox.open(&open).await.expect_err("a fault");

    assert!(!error.is_refusal());
    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn seed_when_the_hash_patch_fails_the_pod_is_deleted_and_open_is_a_port_fault() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes
        .cluster
        .fail_patches_touching(&PodKeys::default().seed_hash());
    let lineage = Lineage::new();

    let error = boxes
        .sandbox
        .open(&fresh_request(lineage.first()))
        .await
        .expect_err("a fault");

    assert!(!error.is_refusal());
    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
}
