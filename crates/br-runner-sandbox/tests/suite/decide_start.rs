use std::time::{Duration, UNIX_EPOCH};

use br_agentic_harness_contract::FreshReason;
use br_runner_sandbox::{
    AttemptId, ClientId, Manifest, PartReport, Record, RecordError, ReleasePlan, RestoreRecheck,
    SaveKey, Sha256Hex, StartPlan, StoreView, Timestamp, Tombstone, live_expired, plan_start,
    recheck_after_restore, release_plan, restore_failed,
};

use crate::common::{agent, thread, turn};

const TTL: Duration = Duration::from_secs(3600);

fn at(seconds: u64) -> Timestamp {
    Timestamp::from_system_time(UNIX_EPOCH + Duration::from_secs(seconds))
}

fn manifest(made_at: Timestamp) -> Manifest {
    Manifest::new(
        SaveKey::new(
            ClientId::new("acme").expect("client"),
            thread(),
            agent(),
            turn(),
        ),
        None,
        AttemptId::mint(),
        made_at,
        vec!["/work".to_owned()],
        PartReport {
            entries: 1,
            bytes_raw: 10,
            bytes_stored: 5,
            sha256_raw: Sha256Hex::of(b"a"),
            sha256_stored: Sha256Hex::of(b"b"),
        },
    )
}

fn plan(view: StoreView, now: u64) -> StartPlan {
    plan_start(view, turn(), at(now), TTL)
}

#[test]
fn row_1_a_manifest_in_time_is_restored() {
    let saved = manifest(at(1000));
    assert_eq!(
        plan(StoreView::Manifest(Box::new(saved.clone())), 4599),
        StartPlan::Restore(Box::new(saved))
    );
}

#[test]
fn row_2_a_manifest_with_made_at_plus_x_passed_is_save_expired_with_that_time() {
    let saved = manifest(at(1000));
    assert_eq!(
        plan(StoreView::Manifest(Box::new(saved)), 4600),
        StartPlan::Fresh(FreshReason::SaveExpired {
            expired_at: Some(UNIX_EPOCH + Duration::from_secs(4600))
        })
    );
}

#[test]
fn row_3_a_tombstone_is_save_expired_with_its_time() {
    let tombstone = Tombstone::new("t", at(10), at(500));
    assert_eq!(
        plan(StoreView::Tombstone(tombstone), 100),
        StartPlan::Fresh(live_expired(at(500)))
    );
}

#[test]
fn row_4_a_record_started_or_failed_is_save_failed_with_the_parent() {
    let parent = turn();
    let started = Record::started(AttemptId::mint(), at(1));
    let failed = started.failed(at(2), RecordError::Store);
    for record in [started, failed] {
        assert_eq!(
            plan_start(StoreView::Record(record), parent, at(100), TTL),
            StartPlan::Fresh(FreshReason::SaveFailed { turn: parent })
        );
    }
}

#[test]
fn row_5_a_record_done_is_save_expired_with_no_time() {
    let done = Record::started(AttemptId::mint(), at(1)).done(at(2));
    assert_eq!(
        plan(StoreView::Record(done), 100),
        StartPlan::Fresh(FreshReason::SaveExpired { expired_at: None })
    );
}

#[test]
fn row_6_a_failed_restore_is_restore_failed_with_the_parent() {
    let parent = turn();
    assert_eq!(
        restore_failed(parent),
        FreshReason::RestoreFailed { turn: parent }
    );
}

#[test]
fn row_7_none_of_the_three_objects_is_no_save_ever() {
    assert_eq!(
        plan(StoreView::Nothing, 1),
        StartPlan::Fresh(FreshReason::NoSaveEver)
    );
}

#[test]
fn row_8_a_store_that_fails_is_store_unavailable() {
    assert_eq!(
        plan(StoreView::Unavailable, 1),
        StartPlan::Fresh(FreshReason::StoreUnavailable)
    );
}

#[test]
fn an_expired_save_is_never_a_restore_whatever_the_bucket_still_holds() {
    let saved = manifest(at(0));
    assert!(matches!(
        plan(StoreView::Manifest(Box::new(saved)), 1_000_000),
        StartPlan::Fresh(FreshReason::SaveExpired { .. })
    ));
}

#[test]
fn the_manifest_read_again_after_the_parts_keeps_the_start_when_it_is_in_time() {
    let first = manifest(at(1000));
    let second = manifest(at(1000));
    assert_eq!(
        recheck_after_restore(&first, Some(&second), at(2000), TTL),
        RestoreRecheck::Keep
    );
}

#[test]
fn a_manifest_that_is_gone_or_expired_after_the_parts_gives_save_expired_with_the_time_of_the_first_read()
 {
    let first = manifest(at(1000));
    let expected = RestoreRecheck::Expired(live_expired(at(4600)));
    assert_eq!(recheck_after_restore(&first, None, at(2000), TTL), expected);
    let second = manifest(at(1000));
    assert_eq!(
        recheck_after_restore(&first, Some(&second), at(4600), TTL),
        expected
    );
}

#[test]
fn a_saved_box_with_bytes_at_the_threshold_is_kept_and_below_it_or_unsaved_is_deleted() {
    assert_eq!(
        release_plan(Some(10_000_000), 10_000_000),
        ReleasePlan::KeepIdle
    );
    assert_eq!(
        release_plan(Some(9_999_999), 10_000_000),
        ReleasePlan::Delete
    );
    assert_eq!(release_plan(None, 10_000_000), ReleasePlan::Delete);
}
