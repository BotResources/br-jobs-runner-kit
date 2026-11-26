use std::time::Duration;

use br_agentic_harness_contract::{
    BoxId, FilePath, FreshReason, SandboxPort, Seed, SeedEntry, StartKind,
};
use br_runner_sandbox::{
    AttemptId, Record, SaveKey, Tombstone, argv, manifest_key, record_key, tombstone_key,
};

use crate::fakes::{
    Boxes, Lineage, Script, boxes, fresh_request, request, staged_save, start_time, warm_pod,
};

fn parent_key(
    boxes: &Boxes,
    lineage: &Lineage,
    parent: br_agentic_harness_contract::TurnId,
) -> SaveKey {
    SaveKey::new(
        boxes.config.client_id().clone(),
        lineage.thread,
        lineage.agent,
        parent,
    )
}

fn reads(boxes: &Boxes) -> Vec<String> {
    boxes
        .store
        .log()
        .into_iter()
        .filter(|line| line.starts_with("get "))
        .collect()
}

type Expected = fn(br_agentic_harness_contract::TurnId) -> FreshReason;

fn two_warm_pods(boxes: &Boxes) {
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.add_pod(warm_pod("warm-main-b"));
}

#[tokio::test(start_paused = true)]
async fn start_a_save_in_time_is_restored_into_a_claimed_pod_and_the_manifest_is_read_again_after_the_parts()
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

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Restored { turn: parent });
    assert_eq!(opened.box_id, BoxId::new("warm-main-a"));
    let execs = boxes.cluster.scripts.execs();
    assert_eq!(execs.len(), 1);
    assert_eq!(execs[0].request.container, "sandbox");
    assert_eq!(
        execs[0].request.command,
        argv::tar_restore(boxes.config.tools_path())
    );
    let manifest_reads = reads(&boxes)
        .into_iter()
        .filter(|line| line.ends_with(&manifest_key(manifest.key())))
        .count();
    assert_eq!(manifest_reads, 2);
}

#[tokio::test(start_paused = true)]
async fn start_the_restore_and_the_seed_are_written_to_the_same_pod_in_two_containers() {
    let boxes = boxes();
    two_warm_pods(&boxes);
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    staged_save(&boxes, &lineage, parent);
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

    assert_eq!(opened.start, StartKind::Restored { turn: parent });
    let mut containers: Vec<_> = boxes
        .cluster
        .scripts
        .execs()
        .iter()
        .map(|exec| (exec.request.pod.clone(), exec.request.container.clone()))
        .collect();
    containers.sort();
    assert_eq!(
        containers,
        vec![
            ("warm-main-a".to_owned(), "sandbox".to_owned()),
            ("warm-main-a".to_owned(), "seed".to_owned()),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn start_a_manifest_whose_time_passed_gives_fresh_save_expired_with_no_restore() {
    let boxes = boxes();
    two_warm_pods(&boxes);
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    staged_save(&boxes, &lineage, parent);
    boxes.clock.advance(Duration::from_secs(3601));

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
    assert!(boxes.cluster.scripts.execs().is_empty());
}

#[tokio::test(start_paused = true)]
async fn start_a_tombstone_gives_fresh_save_expired_and_the_record_is_not_read() {
    let boxes = boxes();
    two_warm_pods(&boxes);
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    let key = parent_key(&boxes, &lineage, parent);
    let expired_at = start_time().plus(Duration::from_secs(60));
    boxes.store.insert(
        &tombstone_key(&key),
        Tombstone::new(parent, start_time(), expired_at)
            .to_vec()
            .expect("bytes"),
    );

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    assert_eq!(
        opened.start,
        StartKind::Fresh(FreshReason::SaveExpired {
            expired_at: Some(expired_at.to_system_time())
        })
    );
    assert_eq!(reads(&boxes).len(), 2);
}

#[tokio::test(start_paused = true)]
async fn start_a_failed_or_started_record_gives_fresh_save_failed_and_a_done_record_gives_save_expired_with_no_time()
 {
    let cases: [(Record, Expected); 2] = [
        (Record::started(AttemptId::mint(), start_time()), |parent| {
            FreshReason::SaveFailed { turn: parent }
        }),
        (
            Record::started(AttemptId::mint(), start_time()).done(start_time()),
            |_| FreshReason::SaveExpired { expired_at: None },
        ),
    ];
    for (record, expected) in cases {
        let boxes = boxes();
        two_warm_pods(&boxes);
        let lineage = Lineage::new();
        let parent = crate::common::turn();
        let key = parent_key(&boxes, &lineage, parent);
        boxes
            .store
            .insert(&record_key(&key), record.to_vec().expect("bytes"));

        let opened = boxes
            .sandbox
            .open(&fresh_request(lineage.after(parent)))
            .await
            .expect("a box");

        assert_eq!(opened.start, StartKind::Fresh(expected(parent)));
        assert_eq!(reads(&boxes).len(), 3);
    }
}

#[tokio::test(start_paused = true)]
async fn start_none_of_the_three_objects_gives_fresh_no_save_ever_after_three_reads() {
    let boxes = boxes();
    two_warm_pods(&boxes);
    let lineage = Lineage::new();

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(crate::common::turn())))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Fresh(FreshReason::NoSaveEver));
    assert_eq!(reads(&boxes).len(), 3);
}

#[tokio::test(start_paused = true)]
async fn start_a_read_of_the_store_that_fails_gives_fresh_store_unavailable_and_not_an_error() {
    let boxes = boxes();
    two_warm_pods(&boxes);
    let lineage = Lineage::new();
    boxes.store.fail("get", "saves/");

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(crate::common::turn())))
        .await
        .expect("a box");

    assert_eq!(
        opened.start,
        StartKind::Fresh(FreshReason::StoreUnavailable)
    );
}

#[tokio::test(start_paused = true)]
async fn start_a_save_of_another_thread_or_another_agent_is_never_read() {
    let boxes = boxes();
    two_warm_pods(&boxes);
    let (mine, other) = (Lineage::new(), Lineage::new());
    let parent = crate::common::turn();
    staged_save(&boxes, &other, parent);
    staged_save(
        &boxes,
        &Lineage {
            thread: mine.thread,
            agent: crate::common::agent(),
        },
        parent,
    );

    let opened = boxes
        .sandbox
        .open(&fresh_request(mine.after(parent)))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Fresh(FreshReason::NoSaveEver));
    let key = parent_key(&boxes, &mine, parent);
    assert!(reads(&boxes).iter().all(|line| line.contains(&format!(
        "/{}/{}/{}",
        key.thread(),
        key.agent(),
        parent
    ))));
}

#[tokio::test(start_paused = true)]
async fn start_a_save_of_a_later_turn_is_not_used_when_the_parent_is_an_earlier_turn() {
    let boxes = boxes();
    two_warm_pods(&boxes);
    let lineage = Lineage::new();
    let (earlier, later) = (crate::common::turn(), crate::common::turn());
    staged_save(&boxes, &lineage, later);

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(earlier)))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Fresh(FreshReason::NoSaveEver));
}
