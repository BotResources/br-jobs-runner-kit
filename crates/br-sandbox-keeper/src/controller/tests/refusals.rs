use super::*;

const NAMING: &str = "refused by the API server";

fn ready_message(bed: &Bed) -> String {
    bed.message("Ready").unwrap()
}

fn names(bed: &Bed, call: &str) -> bool {
    let message = ready_message(bed);
    message.contains(NAMING) && message.contains(call)
}

fn not_named(bed: &Bed) -> bool {
    !ready_message(bed).contains(NAMING)
}

async fn with_a_stale_replicaset() -> Bed {
    let bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.kube.put_replica_set(owned_replica_set(NS, 1));
    bed
}

#[tokio::test(start_paused = true)]
async fn a_refused_create_of_the_replicaset_is_logged_counted_and_named_in_the_status() {
    let (logs, _guard) = crate::testing::logs::capture();
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.kube.forbid("create_replica_set");
    bed.sync().await;
    assert_eq!(
        bed.condition("Ready"),
        Some(("False".to_owned(), "ReplicaSetAbsent".to_owned()))
    );
    assert!(names(&bed, "create_replica_set"));
    assert!(bed.errors() >= 1);
    let logged = logs
        .text()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|line| {
            line["level"] == "WARN"
                && line["fields"]["message"] == "the API server refused the call"
                && line["fields"]["call"] == "create_replica_set"
        });
    assert!(logged);
    bed.kube.permit("create_replica_set");
    bed.pass(10).await;
    assert!(bed.kube.raw_replica_set(NS, RS).is_some());
    assert!(not_named(&bed));
}

#[tokio::test(start_paused = true)]
async fn a_refused_write_of_the_replicas_is_named_until_it_is_allowed() {
    let mut bed = with_a_stale_replicaset().await;
    bed.kube.forbid("apply_replicas");
    bed.sync().await;
    bed.pass(10).await;
    assert!(names(&bed, "apply_replicas"));
    bed.kube.permit("apply_replicas");
    bed.pass(10).await;
    assert_eq!(bed.replicas(), Some(2));
    assert!(not_named(&bed));
}

#[tokio::test(start_paused = true)]
async fn a_refused_write_of_the_template_is_named_until_it_is_allowed() {
    let mut bed = with_a_stale_replicaset().await;
    bed.kube.forbid("apply_template");
    bed.sync().await;
    bed.pass(10).await;
    assert!(names(&bed, "apply_template"));
    bed.kube.permit("apply_template");
    bed.pass(10).await;
    assert!(bed.template_hash().is_some());
    assert!(not_named(&bed));
}

#[tokio::test(start_paused = true)]
async fn a_refused_write_of_the_condition_is_named_until_it_is_allowed() {
    let mut bed = bed(true);
    bed.kube.put_pod(old_warm("w0", 10));
    bed.kube.forbid("apply_condition");
    bed.sync().await;
    bed.pass(10).await;
    assert!(names(&bed, "apply_condition"));
    bed.kube.permit("apply_condition");
    bed.pass(10).await;
    assert!(not_named(&bed));
}

#[tokio::test(start_paused = true)]
async fn a_refused_delete_of_a_pod_is_named_until_the_pod_is_deleted() {
    let mut bed = bed(true);
    bed.kube.forbid("delete_pod");
    bed.kube.put_pod(old_warm("w0", 2000));
    bed.sync().await;
    bed.pass(10).await;
    assert!(bed.kube.pod(NS, "w0").is_some());
    assert!(names(&bed, "delete_pod"));
    bed.kube.permit("delete_pod");
    bed.pass(10).await;
    assert!(bed.kube.pod(NS, "w0").is_none());
    assert!(not_named(&bed));
}

#[tokio::test(start_paused = true)]
async fn refused_deletes_of_several_pods_are_named_once_with_their_number() {
    let mut bed = bed(true);
    bed.kube.forbid("delete_pod");
    bed.kube.put_pod(old_warm("w0", 2000));
    bed.kube.put_pod(old_warm("w1", 2000));
    bed.sync().await;
    bed.pass(10).await;
    let message = ready_message(&bed);
    assert_eq!(message.matches(NAMING).count(), 1);
    assert!(message.contains("delete_pod for 2 objects"));
}

#[tokio::test(start_paused = true)]
async fn a_refused_pod_that_goes_away_is_no_longer_named() {
    let mut bed = bed(true);
    bed.kube.forbid("delete_pod");
    bed.kube.put_pod(old_warm("w0", 2000));
    bed.sync().await;
    bed.pass(10).await;
    assert!(names(&bed, "delete_pod"));
    bed.kube.remove_pod(NS, "w0");
    bed.pass(10).await;
    assert!(not_named(&bed));
}

#[tokio::test(start_paused = true)]
async fn a_refused_delete_of_the_replicaset_of_a_namespace_without_fence_is_named() {
    let mut bed = bed(true);
    bed.sync().await;
    bed.kube.remove_quota(NS);
    bed.kube.forbid("delete_replica_set");
    let bucket = bed.bucket();
    bed.pass(bucket + 1).await;
    assert!(bed.kube.raw_replica_set(NS, RS).is_some());
    assert!(names(&bed, "delete_replica_set"));
    assert_eq!(
        bed.condition("Accepted"),
        Some(("False".to_owned(), "NamespaceNotFenced".to_owned()))
    );
}

#[tokio::test(start_paused = true)]
async fn nothing_is_written_when_the_write_of_the_status_is_refused_too() {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.kube.forbid("create_replica_set");
    bed.kube.forbid("patch_pool_status");
    bed.sync().await;
    bed.pass(10).await;
    assert!(bed.status().is_none());
    assert!(bed.kube.raw_replica_set(NS, RS).is_none());
    assert!(bed.errors() >= 2);
    bed.kube.permit("patch_pool_status");
    bed.pass(10).await;
    assert!(names(&bed, "create_replica_set"));
}

#[tokio::test(start_paused = true)]
async fn a_failure_that_is_not_a_refusal_is_not_named() {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.kube.fail("create_replica_set");
    bed.sync().await;
    bed.pass(10).await;
    assert!(bed.errors() >= 1);
    assert!(not_named(&bed));
}
