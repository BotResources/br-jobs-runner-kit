use crate::facts::ConditionFacts;

use super::*;

fn unknown(bed: &Bed, kind: &str) -> bool {
    bed.condition(kind) == Some(("Unknown".to_owned(), "NotObserved".to_owned()))
}

async fn faulted(operation: &'static str) -> Bed {
    let mut bed = bed(true);
    bed.kube.fail(operation);
    bed.sync().await;
    bed.pass(30).await;
    bed
}

async fn assert_nothing_written(bed: &Bed) {
    let writes: Vec<Call> = bed
        .pool_writes()
        .into_iter()
        .filter(|call| !matches!(call, Call::Condition { .. }))
        .collect();
    assert!(writes.is_empty(), "{writes:?}");
}

#[tokio::test(start_paused = true)]
async fn a_failed_read_of_the_quota_writes_nothing_and_the_status_says_not_observed() {
    let bed = faulted("read_quota").await;
    assert_nothing_written(&bed).await;
    assert!(unknown(&bed, "Accepted"));
    assert!(bed.errors() >= 1);
    assert!(bed.kube.raw_replica_set(NS, RS).is_some());
}

#[tokio::test(start_paused = true)]
async fn a_failed_list_of_the_network_policies_writes_nothing_and_never_deletes_the_replicaset() {
    let bed = faulted("list_network_policies").await;
    assert_nothing_written(&bed).await;
    assert_eq!(bed.replicaset_deletes(), 0);
    assert!(unknown(&bed, "Accepted"));
}

#[tokio::test(start_paused = true)]
async fn a_failed_read_of_the_template_writes_nothing_and_the_status_says_not_observed() {
    let bed = faulted("read_template").await;
    assert_nothing_written(&bed).await;
    assert!(unknown(&bed, "Accepted"));
}

#[tokio::test(start_paused = true)]
async fn a_failed_read_of_the_replicaset_writes_nothing_and_ready_is_not_observed() {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.kube.fail("read_replica_set");
    bed.sync().await;
    bed.pass(30).await;
    assert_nothing_written(&bed).await;
    assert!(bed.kube.raw_replica_set(NS, RS).is_none());
    assert!(unknown(&bed, "Ready"));
}

#[tokio::test(start_paused = true)]
async fn a_failed_read_with_the_fence_lost_deletes_nothing() {
    let mut bed = bed(true);
    bed.sync().await;
    bed.kube.clear_calls();
    bed.kube.remove_quota(NS);
    bed.kube.fail("read_quota");
    let bucket = bed.bucket();
    bed.pass(bucket + 1).await;
    assert_eq!(bed.replicaset_deletes(), 0);
    assert!(bed.kube.raw_replica_set(NS, RS).is_some());
}

#[tokio::test(start_paused = true)]
async fn a_pod_view_that_is_not_synced_writes_nothing_and_the_status_says_not_observed() {
    let mut bed = bed(true);
    bed.sync().await;
    bed.kube.remove_quota(NS);
    bed.kube.clear_calls();
    bed.kube.break_watch("pods", NS, "the pod watch broke");
    bed.pass(6).await;
    assert_nothing_written(&bed).await;
    assert!(unknown(&bed, "Accepted"));
    assert!(unknown(&bed, "Ready"));
}

#[tokio::test(start_paused = true)]
async fn a_namespace_view_that_is_not_synced_writes_nothing() {
    let mut bed = bed(true);
    bed.sync().await;
    bed.kube.remove_quota(NS);
    bed.kube.clear_calls();
    bed.kube
        .break_watch("namespaces", "", "the namespace watch broke");
    bed.settle().await;
    assert!(bed.kube.calls().is_empty());
    assert_eq!(bed.replicaset_deletes(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_binding_view_that_is_not_synced_deletes_no_replicaset() {
    let mut bed = bed(true);
    bed.sync().await;
    bed.kube.remove_binding("br-sandbox-pod");
    bed.kube.clear_calls();
    bed.kube
        .break_watch("bindings", "", "the binding watch broke");
    bed.settle().await;
    assert_eq!(bed.replicaset_deletes(), 0);
    assert!(bed.kube.raw_replica_set(NS, RS).is_some());
}

#[tokio::test(start_paused = true)]
async fn a_pool_view_that_is_not_synced_writes_nothing_for_the_resource() {
    let mut bed = bed(true);
    bed.sync().await;
    bed.kube.clear_calls();
    bed.kube.break_watch("pools", NS, "the pool watch broke");
    bed.settle().await;
    assert!(bed.kube.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_failed_create_is_counted_the_status_says_replicaset_absent_and_the_next_pass_makes_it() {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.kube.fail("create_replica_set");
    bed.sync().await;
    assert!(bed.errors() >= 1);
    assert!(bed.kube.raw_replica_set(NS, RS).is_none());
    assert_eq!(
        bed.condition("Ready"),
        Some(("False".to_owned(), "ReplicaSetAbsent".to_owned()))
    );
    bed.kube.heal("create_replica_set");
    bed.pass(10).await;
    assert!(bed.kube.raw_replica_set(NS, RS).is_some());
}

#[tokio::test(start_paused = true)]
async fn a_refused_write_of_403_is_retried_until_it_is_allowed() {
    for operation in ["create_replica_set", "apply_template", "apply_replicas"] {
        let mut bed = bare(true);
        bed.kube.put_pool(NS, main_pool());
        if operation != "create_replica_set" {
            bed.kube.put_replica_set(owned_replica_set(NS, 1));
        }
        bed.kube.forbid(operation);
        bed.sync().await;
        bed.pass(10).await;
        let before = bed.kube.calls().len();
        bed.kube.permit(operation);
        bed.pass(10).await;
        assert!(bed.kube.calls().len() > before, "{operation}");
        match operation {
            "create_replica_set" => assert!(bed.kube.raw_replica_set(NS, RS).is_some()),
            "apply_template" => assert!(bed.template_hash().is_some()),
            _ => assert_eq!(bed.replicas(), Some(2)),
        }
    }
}

#[tokio::test(start_paused = true)]
async fn a_failed_write_of_the_status_is_tried_again() {
    let mut bed = bed(true);
    bed.kube.fail("patch_pool_status");
    bed.sync().await;
    assert!(bed.status().is_none());
    bed.kube.heal("patch_pool_status");
    bed.pass(10).await;
    assert!(bed.status().is_some());
}

#[tokio::test(start_paused = true)]
async fn a_failed_event_is_counted_and_tried_again() {
    let mut bed = bed(true);
    bed.kube.put_pod(crate::facts::PodFacts {
        phase: crate::facts::Phase::Pending,
        ready: false,
        ..old_warm("p0", 100)
    });
    bed.kube.fail("create_event");
    bed.sync().await;
    bed.pass(100).await;
    assert!(bed.errors() >= 1);
    bed.kube.heal("create_event");
    bed.pass(10).await;
    assert!(bed.kube.calls().iter().any(|call| matches!(
        call,
        Call::Event {
            reason: "WarmPodsPending",
            ..
        }
    )));
}

#[tokio::test(start_paused = true)]
async fn a_refusal_of_the_pods_is_shown_in_ready_and_no_pod_is_deleted() {
    let mut bed = bed(true);
    bed.set_pool_conditions(vec![ConditionFacts {
        kind: "ReplicaFailure".to_owned(),
        status: "True".to_owned(),
        reason: Some("FailedCreate".to_owned()),
        message: Some("pods are forbidden: exceeded quota".to_owned()),
        last_transition_time: Some(at(START)),
    }]);
    bed.sync().await;
    assert_eq!(
        bed.condition("Ready"),
        Some(("False".to_owned(), "PodsRefused".to_owned()))
    );
    assert!(bed.message("Ready").unwrap().contains("exceeded quota"));
    assert!(bed.deletes().is_empty());
}
