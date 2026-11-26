use std::time::Duration;

use br_agentic_harness_contract::{
    BoxId, FilePath, FreshReason, SandboxPort, Seed, SeedEntry, StartKind,
};
use br_runner_sandbox::manifest_key;

use crate::fakes::{
    Boxes, Lineage, Script, boxes, fresh_request, request, staged_save, start_time, warm_pod,
};

fn two_warm_pods(boxes: &Boxes) {
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.add_pod(warm_pod("warm-main-b"));
}

#[tokio::test(start_paused = true)]
async fn restore_failure_the_pod_is_deleted_a_new_pod_is_claimed_and_the_start_is_fresh_restore_failed()
 {
    let boxes = boxes();
    two_warm_pods(&boxes);
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    staged_save(&boxes, &lineage, parent);
    boxes.cluster.scripts.push_script(Script::exit(2));

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    assert_eq!(
        opened.start,
        StartKind::Fresh(FreshReason::RestoreFailed { turn: parent })
    );
    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
    assert_eq!(opened.box_id, BoxId::new("warm-main-b"));
    assert!(
        boxes
            .cluster
            .scripts
            .execs()
            .iter()
            .all(|exec| exec.request.pod == "warm-main-a")
    );
}

#[tokio::test(start_paused = true)]
async fn restore_failure_the_seed_is_written_on_the_new_pod() {
    let boxes = boxes();
    two_warm_pods(&boxes);
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    staged_save(&boxes, &lineage, parent);
    boxes.cluster.scripts.push_script(Script::exit(2));
    boxes
        .cluster
        .scripts
        .push_script(Script::like_bsdtar_extract());
    boxes
        .cluster
        .scripts
        .push_script(Script::like_bsdtar_extract());
    let entry = SeedEntry::file(FilePath::new("a.md").expect("a path"), 0o644, b"x".to_vec());
    let open = request(
        lineage.after(parent),
        Seed::new(vec![entry]).expect("a seed"),
    );

    let opened = boxes.sandbox.open(&open).await.expect("a box");

    assert_eq!(opened.box_id, BoxId::new("warm-main-b"));
    let on_new: Vec<_> = boxes
        .cluster
        .scripts
        .execs()
        .into_iter()
        .filter(|exec| exec.request.pod == "warm-main-b")
        .collect();
    assert_eq!(on_new.len(), 1);
    assert_eq!(on_new[0].request.container, "seed");
}

#[tokio::test(start_paused = true)]
async fn restore_recheck_a_manifest_that_is_gone_after_the_parts_gives_save_expired_and_the_pod_is_not_used()
 {
    let boxes = boxes();
    two_warm_pods(&boxes);
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    let manifest = staged_save(&boxes, &lineage, parent);
    boxes
        .cluster
        .scripts
        .push_script(Script::like_bsdtar_extract());
    let store = boxes.store.clone();
    let key = manifest_key(manifest.key());
    boxes.store.on_stream(move || store.remove(&key));

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    let expired_at = start_time()
        .plus(Duration::from_secs(3600))
        .to_system_time();
    assert_eq!(
        opened.start,
        StartKind::Fresh(FreshReason::SaveExpired {
            expired_at: Some(expired_at)
        })
    );
    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
    assert_eq!(opened.box_id, BoxId::new("warm-main-b"));
}
