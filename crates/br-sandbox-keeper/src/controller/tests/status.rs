use super::*;

fn field(bed: &Bed, key: &str) -> serde_json::Value {
    bed.status().unwrap()[key].clone()
}

#[tokio::test(start_paused = true)]
async fn the_first_status_has_the_conditions_the_counts_the_generation_and_the_age() {
    let mut bed = bed(true);
    bed.kube.put_pod(old_warm("w0", 10));
    bed.sync().await;
    assert_eq!(
        bed.condition("Accepted"),
        Some(("True".to_owned(), "Valid".to_owned()))
    );
    assert_eq!(
        bed.condition("Ready"),
        Some(("True".to_owned(), "WarmPodsReady".to_owned()))
    );
    assert_eq!(
        bed.condition("WarmPodsFailing"),
        Some(("False".to_owned(), "NoWarmPodFailing".to_owned()))
    );
    assert_eq!(field(&bed, "observedGeneration"), 1);
    assert_eq!(field(&bed, "desiredReplicas"), 2);
    assert_eq!(field(&bed, "replicas"), 1);
    assert_eq!(field(&bed, "readyReplicas"), 1);
    assert!(field(&bed, "observedAt").is_string());
}

#[tokio::test(start_paused = true)]
async fn ready_is_false_below_the_minimum_and_true_with_the_minimum_of_ready_pods() {
    let mut bed = bed(true);
    bed.sync().await;
    assert_eq!(
        bed.condition("Ready"),
        Some(("False".to_owned(), "WarmBelowMinimum".to_owned()))
    );
    bed.kube.put_pod(old_warm("w0", 10));
    bed.pass(10).await;
    assert_eq!(
        bed.condition("Ready"),
        Some(("True".to_owned(), "WarmPodsReady".to_owned()))
    );
}

#[tokio::test(start_paused = true)]
async fn the_status_is_written_only_when_a_value_changes() {
    let mut bed = bed(true);
    bed.sync().await;
    assert_eq!(bed.status_writes(), 1);
    bed.kube.clear_calls();
    bed.pass(120).await;
    assert_eq!(bed.status_writes(), 0);
}

#[tokio::test(start_paused = true)]
async fn the_status_is_written_at_most_once_in_five_seconds() {
    let mut bed = bed(true);
    bed.sync().await;
    bed.kube.clear_calls();
    bed.kube.put_pod(old_warm("w0", 10));
    bed.settle().await;
    assert_eq!(bed.status_writes(), 0);
    bed.pass(3).await;
    assert_eq!(bed.status_writes(), 0);
    bed.pass(3).await;
    assert_eq!(bed.status_writes(), 1);
    assert_eq!(field(&bed, "replicas"), 1);
}

#[tokio::test(start_paused = true)]
async fn a_bad_edit_freezes_the_pool_and_the_expiry_of_pods_goes_on() {
    let mut bed = bed(true);
    bed.kube.put_pod(old_warm("w0", 10));
    bed.kube.put_pod(old_warm("w1", 1799));
    bed.sync().await;
    let hash = bed.template_hash();
    let replicas = bed.replicas();
    bed.kube.clear_calls();
    let mut broken = spec();
    broken.min_replicas = 30;
    broken.image = "registry.example/agent:9".to_owned();
    bed.kube.put_pool(NS, edited(&broken, 2));
    bed.pass(5).await;
    assert_eq!(
        bed.condition("Accepted"),
        Some(("False".to_owned(), "InvalidSpec".to_owned()))
    );
    assert_eq!(field(&bed, "observedGeneration"), 2);
    assert_eq!(bed.template_hash(), hash);
    assert_eq!(bed.replicas(), replicas);
    assert_eq!(bed.template_writes(), 0);
    assert!(bed.replica_writes().is_empty());
    assert!(bed.kube.pod(NS, "w1").is_none());
    assert!(bed.kube.pod(NS, "w0").is_some());
}

#[tokio::test(start_paused = true)]
async fn a_good_edit_after_a_freeze_serves_again() {
    let mut bed = bed(true);
    bed.sync().await;
    let mut broken = spec();
    broken.min_replicas = 30;
    bed.kube.put_pool(NS, edited(&broken, 2));
    bed.pass(6).await;
    let mut good = spec();
    good.image = "registry.example/agent:3".to_owned();
    bed.kube.put_pool(NS, edited(&good, 3));
    bed.pass(6).await;
    assert_eq!(
        bed.condition("Accepted"),
        Some(("True".to_owned(), "Valid".to_owned()))
    );
    assert_eq!(field(&bed, "observedGeneration"), 3);
}

#[tokio::test(start_paused = true)]
async fn the_status_of_a_pool_that_is_not_observed_keeps_the_last_judged_generation() {
    let mut bed = bed(true);
    bed.sync().await;
    bed.kube.fail("read_quota");
    let mut edit = spec();
    edit.image = "registry.example/agent:4".to_owned();
    bed.kube.put_pool(NS, edited(&edit, 2));
    bed.pass(6).await;
    assert_eq!(
        bed.condition("Accepted"),
        Some(("Unknown".to_owned(), "NotObserved".to_owned()))
    );
    assert_eq!(field(&bed, "observedGeneration"), 1);
}

#[tokio::test(start_paused = true)]
async fn the_status_stays_as_written_after_the_loss_of_the_leadership() {
    let mut bed = bed(true);
    bed.sync().await;
    let written = bed.status().unwrap();
    bed.leadership.renewed(at(START));
    tokio::time::advance(Duration::from_secs(10)).await;
    bed.kube.put_pod(old_warm("w0", 10));
    bed.pass(30).await;
    assert_eq!(bed.status().unwrap(), written);
}
