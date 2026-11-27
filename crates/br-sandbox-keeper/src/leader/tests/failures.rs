use serde_json::Value;

use super::*;

fn counted(replica: &Replica) -> u64 {
    replica
        .metrics
        .render(false)
        .lines()
        .find_map(|line| line.strip_prefix("br_sandbox_keeper_sweep_errors_total "))
        .and_then(|count| count.parse().ok())
        .expect("the counter is rendered")
}

fn warnings(logs: &crate::testing::logs::Captured) -> Vec<Value> {
    logs.text()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|line| line["level"] == "WARN")
        .collect()
}

fn message(line: &Value) -> &str {
    line["fields"]["message"].as_str().unwrap()
}

fn error_text(line: &Value) -> &str {
    line["fields"]["error"].as_str().unwrap()
}

#[tokio::test(start_paused = true)]
async fn a_failed_lease_read_is_logged_counted_and_tried_again_at_the_retry_period() {
    let (logs, _guard) = crate::testing::logs::capture();
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    kube.fail("get_lease");
    assert_eq!(a.elector.round().await, settings().retry_period);
    assert_eq!(counted(&a), 1);
    assert!(!a.leadership.is_leader());
    assert!(kube.calls().is_empty());
    let lines = warnings(&logs);
    assert_eq!(lines.len(), 1);
    assert_eq!(message(&lines[0]), "the lease could not be read");
    assert!(error_text(&lines[0]).contains("scripted get_lease failure"));
    assert_eq!(a.elector.round().await, settings().retry_period);
    assert_eq!(counted(&a), 2);
    kube.heal("get_lease");
    a.elector.round().await;
    assert_eq!(counted(&a), 2);
    assert!(a.leadership.is_leader());
}

#[tokio::test(start_paused = true)]
async fn a_failed_lease_create_is_logged_counted_and_tried_again_at_the_retry_period() {
    let (logs, _guard) = crate::testing::logs::capture();
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    kube.fail("create_lease");
    assert_eq!(a.elector.round().await, settings().retry_period);
    assert_eq!(counted(&a), 1);
    assert!(!a.leadership.is_leader());
    assert!(kube.lease().is_none());
    let lines = warnings(&logs);
    assert_eq!(lines.len(), 1);
    assert_eq!(message(&lines[0]), "the lease could not be written");
    assert!(error_text(&lines[0]).contains("scripted create_lease failure"));
    kube.heal("create_lease");
    a.elector.round().await;
    assert_eq!(counted(&a), 1);
    assert!(a.leadership.is_leader());
}

#[tokio::test(start_paused = true)]
async fn a_failed_lease_renewal_is_logged_counted_and_keeps_the_leadership_until_its_deadline() {
    let (logs, _guard) = crate::testing::logs::capture();
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    a.elector.round().await;
    assert_eq!(counted(&a), 0);
    kube.fail("update_lease");
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(a.elector.round().await, settings().retry_period);
    assert_eq!(counted(&a), 1);
    assert!(a.leadership.is_leader());
    let lines = warnings(&logs);
    assert_eq!(lines.len(), 1);
    assert_eq!(message(&lines[0]), "the lease could not be written");
    assert!(error_text(&lines[0]).contains("scripted update_lease failure"));
    kube.heal("update_lease");
    tokio::time::sleep(Duration::from_secs(2)).await;
    a.elector.round().await;
    assert_eq!(counted(&a), 1);
    assert_eq!(warnings(&logs).len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_read_and_a_write_that_succeed_and_a_conflict_count_nothing() {
    let (logs, _guard) = crate::testing::logs::capture();
    let (kube, clock) = (Arc::new(FakeKube::default()), clock());
    let a = replica(&kube, &clock, "a");
    let b = replica(&kube, &clock, "b");
    for _ in 0..3 {
        a.elector.round().await;
        b.elector.round().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    kube.conflict_next_lease_writes(1);
    a.elector.round().await;
    assert_eq!(counted(&a), 0);
    assert_eq!(counted(&b), 0);
    assert!(a.leadership.is_leader());
    assert!(warnings(&logs).is_empty());
}
