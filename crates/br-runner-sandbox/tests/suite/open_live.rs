use br_agentic_harness_contract::{BoxId, FreshReason, SandboxPort, StartKind};
use br_runner_sandbox::PodKeys;
use serde_json::{Value, json};

use crate::fakes::{
    Boxes, Lineage, Script, annotate, boxes, fresh_request, live_pod, staged_save, warm_pod,
};

type Spoil = fn(&mut br_runner_sandbox::PodView);

fn keys() -> PodKeys {
    PodKeys::default()
}

fn annotation(boxes: &Boxes, pod: &str, key: String) -> Option<String> {
    boxes.cluster.pod(pod)?.annotations.get(&key).cloned()
}

fn claim_patches(boxes: &Boxes) -> Vec<(String, Value)> {
    boxes
        .cluster
        .patches()
        .into_iter()
        .filter(|(_, patch)| patch.pointer("/metadata/resourceVersion").is_some())
        .collect()
}

#[tokio::test(start_paused = true)]
async fn live_reuse_a_pod_saved_for_the_parent_is_continued_with_no_call_to_the_store_and_no_seed_write()
 {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    boxes.cluster.add_pod(live_pod("live-1", &lineage, parent));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Continued);
    assert_eq!(opened.box_id, BoxId::new("live-1"));
    assert!(boxes.store.log().is_empty());
    assert!(boxes.cluster.scripts.execs().is_empty());
    assert_eq!(claim_patches(&boxes).len(), 1);
}

#[tokio::test(start_paused = true)]
async fn live_reuse_is_one_list_by_the_labels_of_the_thread_and_the_agent_and_one_patch() {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    boxes.cluster.add_pod(live_pod("live-1", &lineage, parent));

    boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    let keys = keys();
    let calls = boxes.cluster.calls();
    assert_eq!(
        calls,
        vec![
            format!(
                "list {}={},{}=claimed,{}={}",
                keys.agent(),
                lineage.agent,
                keys.pool(),
                keys.thread(),
                lineage.thread
            ),
            "patch live-1".to_owned(),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn live_claim_sets_clean_false_and_the_claim_time_removes_idle_since_and_state_bytes_and_keeps_the_save_marks()
 {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    boxes.cluster.add_pod(live_pod("live-1", &lineage, parent));
    let before = boxes.cluster.pod("live-1").expect("a pod");

    boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    let keys = keys();
    let (_, patch) = &claim_patches(&boxes)[0];
    assert_eq!(
        patch.pointer("/metadata/resourceVersion"),
        Some(&json!(before.resource_version))
    );
    assert_eq!(
        annotation(&boxes, "live-1", keys.clean()).as_deref(),
        Some("false")
    );
    assert!(annotation(&boxes, "live-1", keys.claimed_at()).is_some());
    assert_eq!(annotation(&boxes, "live-1", keys.idle_since()), None);
    assert_eq!(annotation(&boxes, "live-1", keys.state_bytes()), None);
    for kept in [keys.state_turn(), keys.saved_at(), keys.seed_hash()] {
        assert_eq!(
            annotation(&boxes, "live-1", kept.clone()),
            before.annotations.get(&kept).cloned(),
            "{kept}"
        );
    }
    let labels = &boxes.cluster.pod("live-1").expect("a pod").labels;
    assert_eq!(labels, &before.labels);
}

fn spoil_state_turn(pod: &mut br_runner_sandbox::PodView) {
    annotate(pod, keys().state_turn(), crate::common::turn().to_string());
}

fn spoil_dirty(pod: &mut br_runner_sandbox::PodView) {
    annotate(pod, keys().clean(), "false");
}

fn spoil_idle_passed(pod: &mut br_runner_sandbox::PodView) {
    annotate(pod, keys().idle_since(), "2026-11-26T09:00:00Z");
}

fn spoil_no_idle(pod: &mut br_runner_sandbox::PodView) {
    pod.annotations.remove(&keys().idle_since());
}

fn spoil_light(pod: &mut br_runner_sandbox::PodView) {
    annotate(pod, keys().state_bytes(), "9999999");
}

fn spoil_seed(pod: &mut br_runner_sandbox::PodView) {
    annotate(pod, keys().seed_hash(), "another");
}

fn spoil_ready(pod: &mut br_runner_sandbox::PodView) {
    pod.ready_condition = false;
}

#[tokio::test(start_paused = true)]
async fn live_reuse_a_pod_that_fails_one_condition_is_not_reused_and_the_start_comes_from_the_store()
 {
    let spoilers: [(&str, Spoil); 7] = [
        ("the save of another turn", spoil_state_turn),
        ("a dirty pod", spoil_dirty),
        ("an idle time that passed", spoil_idle_passed),
        ("no idle time", spoil_no_idle),
        ("a state below the threshold", spoil_light),
        ("another seed hash", spoil_seed),
        ("a pod that is not ready", spoil_ready),
    ];
    for (name, spoil) in spoilers {
        let boxes = boxes();
        let lineage = Lineage::new();
        let parent = crate::common::turn();
        let mut live = live_pod("live-1", &lineage, parent);
        spoil(&mut live);
        boxes.cluster.add_pod(live);
        boxes.cluster.add_pod(warm_pod("warm-main-a"));
        staged_save(&boxes, &lineage, parent);
        boxes
            .cluster
            .scripts
            .push_script(Script::like_bsdtar_extract());

        let opened = boxes
            .sandbox
            .open(&fresh_request(lineage.after(parent)))
            .await
            .expect("a box");

        assert_eq!(opened.start, StartKind::Restored { turn: parent }, "{name}");
        assert_eq!(opened.box_id, BoxId::new("warm-main-a"), "{name}");
        assert!(
            boxes.cluster.pod("live-1").is_some(),
            "{name}: never deleted"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn live_reuse_a_dirty_pod_is_never_deleted_by_open() {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    let mut live = live_pod("live-1", &lineage, parent);
    spoil_dirty(&mut live);
    boxes.cluster.add_pod(live);
    boxes.cluster.add_pod(warm_pod("warm-main-a"));

    boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    assert!(boxes.cluster.deleted().is_empty());
}

#[tokio::test(start_paused = true)]
async fn live_reuse_a_pod_of_another_agent_or_another_thread_is_never_reused() {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    boxes.cluster.add_pod(live_pod(
        "other-agent",
        &Lineage {
            thread: lineage.thread,
            agent: crate::common::agent(),
        },
        parent,
    ));
    boxes.cluster.add_pod(live_pod(
        "other-thread",
        &Lineage {
            thread: crate::common::thread(),
            agent: lineage.agent,
        },
        parent,
    ));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Fresh(FreshReason::NoSaveEver));
    assert_eq!(opened.box_id, BoxId::new("warm-main-a"));
}

#[tokio::test(start_paused = true)]
async fn live_reuse_a_conflict_on_the_claim_makes_the_pod_not_reusable() {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    boxes.cluster.add_pod(live_pod("live-1", &lineage, parent));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.rival_touches("live-1");

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Fresh(FreshReason::NoSaveEver));
    assert_eq!(opened.box_id, BoxId::new("warm-main-a"));
}
