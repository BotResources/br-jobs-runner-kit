use std::time::Duration;

use br_agentic_harness_contract::{BoxId, FreshReason, SandboxPort, StartKind};
use br_runner_sandbox::PodKeys;

use crate::fakes::{Lineage, annotate, boxes, fresh_request, live_pod, start_time, warm_pod};

fn keys() -> PodKeys {
    PodKeys::default()
}

#[tokio::test(start_paused = true)]
async fn live_reuse_an_expired_save_of_the_pod_is_deleted_and_the_start_is_fresh_with_the_time_and_no_read_of_the_store()
 {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    boxes.cluster.add_pod(live_pod("live-1", &lineage, parent));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
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
    assert_eq!(boxes.cluster.deleted(), vec!["live-1".to_owned()]);
    assert!(boxes.store.log().is_empty());
    assert_eq!(opened.box_id, BoxId::new("warm-main-a"));
}

#[tokio::test(start_paused = true)]
async fn live_reuse_a_reusable_candidate_wins_over_an_expired_one() {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    let mut old = live_pod("live-1", &lineage, parent);
    annotate(&mut old, keys().saved_at(), "2026-11-26T07:00:00Z");
    boxes.cluster.add_pod(old);
    boxes.cluster.add_pod(live_pod("live-2", &lineage, parent));

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Continued);
    assert_eq!(opened.box_id, BoxId::new("live-2"));
    assert_eq!(boxes.cluster.deleted(), vec!["live-1".to_owned()]);
}
