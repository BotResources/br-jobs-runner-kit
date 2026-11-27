use super::*;
use crate::bucket::GuardedBucket;
use crate::fakes::{FakeBucket, TokioClock};
use crate::testing::saves::{manifest, part_object, save_key};
use crate::time::at;
use br_runner_sandbox::{
    AttemptId, Record, SaveKey, Tombstone, manifest_key, record_key, tombstone_key,
};

mod aged;
mod more;

const NOW: u64 = 100_000;

struct Harness {
    bucket: Arc<FakeBucket>,
    sweeper: Sweeper,
}

fn rules() -> SweepRules {
    SweepRules {
        save_ttl: Duration::from_secs(1000),
        skew_margin: Duration::from_secs(60),
        attempt_ttl: Duration::from_secs(3600),
        tombstone_ttl: Duration::from_secs(10_000),
    }
}

fn harness(leader: bool, page_size: usize) -> Harness {
    let bucket = Arc::new(FakeBucket::default());
    let clock: Arc<dyn Clock> = Arc::new(TokioClock::starting_at(at(NOW)));
    let leadership = Leadership::new(clock.clone());
    if leader {
        leadership.renewed(at(NOW + 1_000_000));
    }
    let sweeper = Sweeper {
        bucket: Arc::new(GuardedBucket::new(bucket.clone())),
        clock,
        leadership,
        metrics: Arc::new(Metrics::new()),
        rules: rules(),
        page_size,
        interval: Duration::from_secs(60),
    };
    Harness { bucket, sweeper }
}

fn put_save(
    bucket: &FakeBucket,
    key: &SaveKey,
    attempts: &[AttemptId],
    named: AttemptId,
    made: u64,
) {
    let made_manifest = manifest(key, named, at(made));
    let (manifest_name, body) = crate::testing::saves::manifest_object(key, &made_manifest);
    bucket.insert(&manifest_name, &body, made);
    for attempt in attempts {
        bucket.insert(&part_object(key, *attempt), b"part", made);
    }
}

fn metric(harness: &Harness, line: &str) -> bool {
    harness
        .sweeper
        .metrics
        .render(true)
        .lines()
        .any(|text| text == line)
}

fn harness_sweeper(harness: &Harness) -> Sweeper {
    Sweeper {
        bucket: harness.sweeper.bucket.clone(),
        clock: harness.sweeper.clock.clone(),
        leadership: harness.sweeper.leadership.clone(),
        metrics: harness.sweeper.metrics.clone(),
        rules: harness.sweeper.rules,
        page_size: harness.sweeper.page_size,
        interval: harness.sweeper.interval,
    }
}

#[tokio::test(start_paused = true)]
async fn an_expired_save_gets_its_tombstone_then_its_manifest_then_its_parts_and_the_record_stays()
{
    let harness = harness(true, 100);
    let key = save_key();
    let (named, rival) = (AttemptId::mint(), AttemptId::mint());
    put_save(&harness.bucket, &key, &[named, rival], named, NOW - 2000);
    harness.bucket.insert(&record_key(&key), b"{}", NOW - 2000);
    harness.sweeper.sweep_once().await;
    let writes = harness.bucket.writes();
    assert_eq!(writes[0], format!("put {}", tombstone_key(&key)));
    assert_eq!(writes[1], format!("delete {}", manifest_key(&key)));
    assert_eq!(writes.len(), 4);
    assert!(harness.bucket.contains(&tombstone_key(&key)));
    assert!(harness.bucket.contains(&record_key(&key)));
    assert!(!harness.bucket.contains(&manifest_key(&key)));
    assert!(!harness.bucket.contains(&part_object(&key, named)));
    assert!(metric(&harness, "br_sandbox_keeper_saves_deleted_total 1"));
    assert!(metric(
        &harness,
        "br_sandbox_keeper_tombstones_written_total 1"
    ));
}

#[tokio::test(start_paused = true)]
async fn the_tombstone_is_the_same_whenever_the_save_is_swept() {
    let key = save_key();
    let attempt = AttemptId::mint();
    let mut bodies = Vec::new();
    for now in [NOW, NOW + 500] {
        let harness = harness(true, 100);
        put_save(&harness.bucket, &key, &[attempt], attempt, 95_000);
        tokio::time::advance(Duration::from_secs(now - NOW)).await;
        harness.sweeper.sweep_once().await;
        bodies.push(harness.bucket.body(&tombstone_key(&key)).unwrap());
    }
    assert_eq!(bodies[0], bodies[1]);
    let tombstone = Tombstone::parse(&bodies[0]).unwrap();
    assert_eq!(tombstone.expired_at(), at(95_000).plus(rules().save_ttl));
}

#[tokio::test(start_paused = true)]
async fn a_save_inside_the_margin_is_left_whole_and_its_manifest_is_read() {
    let harness = harness(true, 100);
    let key = save_key();
    let attempt = AttemptId::mint();
    put_save(&harness.bucket, &key, &[attempt], attempt, NOW - 1050);
    harness.sweeper.sweep_once().await;
    assert!(harness.bucket.writes().is_empty());
    assert!(harness.bucket.contains(&manifest_key(&key)));
    assert!(harness.bucket.contains(&part_object(&key, attempt)));
    assert_eq!(
        harness.bucket.log(),
        vec![format!("get {}", manifest_key(&key))]
    );
}

#[tokio::test(start_paused = true)]
async fn the_manifest_of_every_listed_save_is_read_once_in_a_sweep_whatever_its_age_and_the_page_size()
 {
    let harness = harness(true, 2);
    let mut manifests = Vec::new();
    for made in [NOW - 10, NOW - 1050, NOW - 2000, NOW - 50_000, NOW - 10] {
        let key = save_key();
        let attempt = AttemptId::mint();
        put_save(&harness.bucket, &key, &[attempt], attempt, made);
        manifests.push(format!("get {}", manifest_key(&key)));
    }
    harness.sweeper.sweep_once().await;
    let log = harness.bucket.log();
    for read in &manifests {
        assert_eq!(log.iter().filter(|line| *line == read).count(), 1);
    }
    let saves_read = log
        .iter()
        .filter(|line| line.starts_with(&format!("get {SAVES_PREFIX}")))
        .count();
    assert_eq!(saves_read, 5);
}

#[tokio::test(start_paused = true)]
async fn a_manifest_that_cannot_be_read_is_never_deleted_for_age() {
    let harness = harness(true, 100);
    let key = save_key();
    let attempt = AttemptId::mint();
    harness.bucket.insert(&manifest_key(&key), b"{", 10);
    harness
        .bucket
        .insert(&part_object(&key, attempt), b"part", 10);
    harness.sweeper.sweep_once().await;
    assert!(harness.bucket.writes().is_empty());
    assert!(metric(&harness, "br_sandbox_keeper_sweep_errors_total 1"));
}

#[tokio::test(start_paused = true)]
async fn an_old_attempt_that_the_manifest_does_not_name_is_deleted_and_counted() {
    let harness = harness(true, 100);
    let key = save_key();
    let (named, loser) = (AttemptId::mint(), AttemptId::mint());
    put_save(&harness.bucket, &key, &[named], named, NOW - 10);
    harness
        .bucket
        .insert(&part_object(&key, loser), b"part", NOW - 5000);
    harness.sweeper.sweep_once().await;
    assert!(!harness.bucket.contains(&part_object(&key, loser)));
    assert!(harness.bucket.contains(&part_object(&key, named)));
    assert!(harness.bucket.contains(&manifest_key(&key)));
    assert!(metric(
        &harness,
        "br_sandbox_keeper_orphans_deleted_total 1"
    ));
}

#[tokio::test(start_paused = true)]
async fn a_turn_with_no_manifest_loses_only_its_old_attempts() {
    let harness = harness(true, 100);
    let key = save_key();
    let (old, live) = (AttemptId::mint(), AttemptId::mint());
    harness
        .bucket
        .insert(&part_object(&key, old), b"part", NOW - 5000);
    harness
        .bucket
        .insert(&part_object(&key, live), b"part", NOW - 10);
    harness.sweeper.sweep_once().await;
    assert!(!harness.bucket.contains(&part_object(&key, old)));
    assert!(harness.bucket.contains(&part_object(&key, live)));
}

#[tokio::test(start_paused = true)]
async fn a_follower_writes_and_deletes_nothing() {
    let harness = harness(false, 100);
    let key = save_key();
    let attempt = AttemptId::mint();
    put_save(&harness.bucket, &key, &[attempt], attempt, NOW - 5000);
    harness.sweeper.sweep_once().await;
    assert!(harness.bucket.writes().is_empty());
    assert!(harness.bucket.list_calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_failed_listing_is_counted_and_ends_the_pass() {
    let harness = harness(true, 100);
    harness.bucket.fail_list();
    harness.sweeper.sweep_once().await;
    assert!(metric(&harness, "br_sandbox_keeper_sweep_errors_total 3"));
}
