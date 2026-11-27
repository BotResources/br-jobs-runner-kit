use k8s_openapi::api::networking::v1::NetworkPolicy;

use crate::cluster::fixtures::named;
use crate::facts::{Mark, PodFacts, fixtures};

use super::*;

fn extra_policy() -> NetworkPolicy {
    named("br-sandbox-extra", |metadata| NetworkPolicy {
        metadata,
        ..NetworkPolicy::default()
    })
}

async fn lost(bed: &mut Bed, loss: impl FnOnce(&Bed)) {
    bed.sync().await;
    bed.kube.clear_calls();
    loss(bed);
    let bucket = bed.bucket();
    bed.pass(bucket + 1).await;
}

fn accepted(bed: &Bed) -> Option<(String, String)> {
    bed.condition("Accepted")
}

fn not_fenced() -> Option<(String, String)> {
    Some(("False".to_owned(), "NamespaceNotFenced".to_owned()))
}

#[tokio::test(start_paused = true)]
async fn a_missing_quota_removes_the_replicaset_and_the_message_names_the_object() {
    let mut bed = bed(true);
    lost(&mut bed, |bed| bed.kube.remove_quota(NS)).await;
    assert_eq!(bed.replicaset_deletes(), 1);
    assert!(bed.kube.raw_replica_set(NS, RS).is_none());
    assert_eq!(accepted(&bed), not_fenced());
    assert!(
        bed.message("Accepted")
            .unwrap()
            .contains("br-sandbox-quota")
    );
    assert_eq!(
        bed.condition("Ready"),
        Some(("False".to_owned(), "ReplicaSetAbsent".to_owned()))
    );
}

#[tokio::test(start_paused = true)]
async fn a_missing_default_deny_policy_removes_the_replicaset() {
    let mut bed = bed(true);
    lost(&mut bed, |bed| {
        bed.kube.put_network_policies(NS, Vec::new())
    })
    .await;
    assert_eq!(bed.replicaset_deletes(), 1);
    assert_eq!(accepted(&bed), not_fenced());
    assert!(
        bed.message("Accepted")
            .unwrap()
            .contains("br-sandbox-default-deny")
    );
}

#[tokio::test(start_paused = true)]
async fn a_network_policy_outside_the_closed_list_removes_the_replicaset_and_is_named() {
    let mut bed = bed(true);
    lost(&mut bed, |bed| {
        bed.kube
            .put_network_policies(NS, vec![deny_all(), extra_policy()]);
    })
    .await;
    assert_eq!(bed.replicaset_deletes(), 1);
    assert_eq!(accepted(&bed), not_fenced());
    assert!(
        bed.message("Accepted")
            .unwrap()
            .contains("br-sandbox-extra")
    );
}

#[tokio::test(start_paused = true)]
async fn a_missing_binding_of_the_admission_policy_removes_the_replicaset_at_once() {
    let mut bed = bed(true);
    bed.sync().await;
    bed.kube.clear_calls();
    bed.kube.remove_binding("br-sandbox-pod");
    bed.pass(6).await;
    assert_eq!(bed.replicaset_deletes(), 1);
    assert_eq!(accepted(&bed), not_fenced());
    assert!(bed.message("Accepted").unwrap().contains("br-sandbox-pod"));
}

#[tokio::test(start_paused = true)]
async fn the_pool_is_made_again_when_the_fence_is_back_and_a_loss_is_no_freeze() {
    let mut bed = bed(true);
    lost(&mut bed, |bed| bed.kube.remove_quota(NS)).await;
    assert!(bed.kube.raw_replica_set(NS, RS).is_none());
    bed.kube.put_quota(NS, quota());
    let bucket = bed.bucket();
    bed.pass(bucket + 1).await;
    assert_eq!(bed.creations().len(), 1);
    assert!(bed.kube.raw_replica_set(NS, RS).is_some());
    assert_eq!(
        accepted(&bed),
        Some(("True".to_owned(), "Valid".to_owned()))
    );
}

#[tokio::test(start_paused = true)]
async fn a_resource_in_a_namespace_with_no_fence_makes_no_replicaset() {
    let mut bed = bare(true);
    bed.kube.remove_quota(NS);
    bed.kube.put_pool(NS, main_pool());
    bed.sync().await;
    assert!(bed.creations().is_empty());
    assert!(bed.kube.raw_replica_set(NS, RS).is_none());
    assert_eq!(accepted(&bed), not_fenced());
}

#[tokio::test(start_paused = true)]
async fn the_loss_of_the_fence_deletes_no_pod_that_is_claimed_or_in_its_turn() {
    let mut bed = bed(true);
    bed.kube.put_pod(PodFacts {
        idle_since: Mark::Value(at(START)),
        state_bytes: Mark::Value(50_000_000),
        ..fixtures::claimed("c0", true, START - 10)
    });
    bed.kube.put_pod(PodFacts {
        claimed_at: Mark::Value(at(START)),
        ..fixtures::claimed("c1", false, START)
    });
    lost(&mut bed, |bed| bed.kube.remove_quota(NS)).await;
    assert!(bed.deletes().is_empty());
    assert!(bed.kube.pod(NS, "c0").is_some());
    assert!(bed.kube.pod(NS, "c1").is_some());
}

#[tokio::test(start_paused = true)]
async fn a_replicaset_that_the_resource_does_not_own_is_not_deleted_when_the_fence_is_lost() {
    let mut bed = bare(true);
    let mut foreign = owned_replica_set(NS, 3);
    foreign.metadata.owner_references = None;
    bed.kube.put_replica_set(foreign);
    bed.kube.put_pool(NS, main_pool());
    bed.kube.remove_quota(NS);
    bed.sync().await;
    assert_eq!(bed.replicaset_deletes(), 0);
    assert!(bed.kube.raw_replica_set(NS, RS).is_some());
}

#[tokio::test(start_paused = true)]
async fn a_follower_deletes_no_replicaset() {
    let mut bed = bed(false);
    bed.kube.remove_quota(NS);
    bed.sync().await;
    bed.pass(30).await;
    assert_eq!(bed.replicaset_deletes(), 0);
    assert!(bed.kube.raw_replica_set(NS, RS).is_some());
}
