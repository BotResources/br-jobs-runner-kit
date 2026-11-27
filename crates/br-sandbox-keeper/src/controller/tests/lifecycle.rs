use k8s_openapi::api::apps::v1::ReplicaSet;
use serde_json::json;

use crate::facts::{Mark, Phase, PodFacts, fixtures};

use super::*;

fn unowned() -> ReplicaSet {
    let mut set = owned_replica_set(NS, 3);
    set.metadata.owner_references = None;
    set
}

fn owned_by_another() -> ReplicaSet {
    let mut set = owned_replica_set(NS, 3);
    if let Some(references) = set.metadata.owner_references.as_mut() {
        references[0].uid = "uid-other".to_owned();
    }
    set
}

#[tokio::test(start_paused = true)]
async fn a_new_resource_makes_the_replicaset_with_its_controller_reference_and_the_sized_replicas()
{
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.sync().await;
    let bodies = bed.creations();
    assert_eq!(bodies.len(), 1);
    let body = &bodies[0];
    assert_eq!(body["metadata"]["name"], RS);
    let reference = &body["metadata"]["ownerReferences"][0];
    assert_eq!(reference["uid"], "uid-1");
    assert_eq!(reference["controller"], true);
    assert!(reference.get("blockOwnerDeletion").is_none());
    assert_eq!(body["spec"]["replicas"], 2);
    assert_eq!(
        body["spec"]["selector"]["matchLabels"]["br.sandbox/pool"],
        "warm"
    );
    assert_eq!(
        body["spec"]["selector"]["matchLabels"]["br.sandbox/pool-id"],
        NS
    );
    assert!(bed.template_hash().is_some());
    assert_eq!(
        bed.condition("Accepted"),
        Some(("True".to_owned(), "Valid".to_owned()))
    );
}

#[tokio::test(start_paused = true)]
async fn nothing_is_made_before_the_views_are_synced() {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.engine.reconcile_all().await;
    bed.engine.reconcile(NS).await;
    assert!(bed.kube.calls().is_empty());
    assert!(bed.kube.raw_replica_set(NS, RS).is_none());
}

#[tokio::test(start_paused = true)]
async fn no_replicaset_is_made_while_the_pod_view_of_the_namespace_is_not_synced() {
    let mut bed = bed(true);
    bed.sync().await;
    bed.kube.remove_replica_set(NS, RS);
    bed.kube.break_watch("pods", NS, "the pod watch broke");
    bed.kube.clear_calls();
    bed.settle().await;
    bed.pass(30).await;
    assert!(bed.creations().is_empty());
    assert!(bed.kube.raw_replica_set(NS, RS).is_none());
    bed.kube.resync_watch("pods", NS);
    bed.settle().await;
    assert_eq!(bed.creations().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_create_that_answers_exists_is_a_new_read_and_never_a_second_object() {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.kube.race_next_create(owned_replica_set(NS, 7));
    bed.sync().await;
    assert_eq!(bed.creations().len(), 1);
    let held = bed.kube.raw_replica_set(NS, RS).unwrap();
    assert_eq!(held.metadata.uid.as_deref(), Some("rs-uid"));
    bed.pass(5).await;
    assert_eq!(bed.creations().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn the_template_is_applied_when_its_hash_differs_and_not_again_when_it_matches() {
    let mut bed = bed(true);
    bed.sync().await;
    assert_eq!(bed.template_writes(), 1);
    let first = bed.template_hash().unwrap();
    bed.kube.clear_calls();
    bed.pass(30).await;
    assert_eq!(bed.template_writes(), 0);
    let mut changed = spec();
    changed.image = "registry.example/agent:2".to_owned();
    bed.kube.put_pool(NS, edited(&changed, 2));
    bed.settle().await;
    assert_eq!(bed.template_writes(), 1);
    assert_ne!(bed.template_hash().unwrap(), first);
}

#[tokio::test(start_paused = true)]
async fn a_change_of_the_configmap_is_seen_within_one_rate_bucket() {
    let mut bed = bed(true);
    bed.sync().await;
    let first = bed.template_hash().unwrap();
    let mut changed = template_json(NS);
    changed["spec"]["containers"][0]["env"] = json!([{"name": "A", "value": "b"}]);
    bed.kube.put_template(NS, template_map(&changed));
    let bucket = bed.engine.config.rate_bucket.as_secs();
    bed.pass(bucket + 1).await;
    assert_ne!(bed.template_hash().unwrap(), first);
}

#[tokio::test(start_paused = true)]
async fn a_replicaset_of_the_name_that_the_resource_does_not_own_is_never_taken_over() {
    for held in [unowned(), owned_by_another()] {
        let mut bed = bare(true);
        bed.kube.put_pool(NS, main_pool());
        bed.kube.put_replica_set(held);
        bed.sync().await;
        bed.pass(30).await;
        assert!(bed.pool_writes().is_empty());
        assert_eq!(bed.replicas(), Some(3));
        assert_eq!(
            bed.condition("Ready"),
            Some(("False".to_owned(), "ReplicaSetNotOwned".to_owned()))
        );
    }
}

#[tokio::test(start_paused = true)]
async fn the_older_resource_wins_and_the_other_is_a_duplicate_that_changes_nothing() {
    let mut bed = bare(true);
    bed.kube.put_pool(NS, main_pool());
    bed.kube
        .put_pool(NS, resource("second", "uid-2", START - 50, &spec()));
    bed.sync().await;
    assert_eq!(bed.creations().len(), 1);
    assert_eq!(
        bed.condition("Accepted"),
        Some(("True".to_owned(), "Valid".to_owned()))
    );
    let second = bed.status_of("second").unwrap();
    let accepted = second["conditions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|condition| condition["type"] == "Accepted")
        .unwrap();
    assert_eq!(accepted["status"], "False");
    assert_eq!(accepted["reason"], "DuplicatePool");
    bed.kube.remove_pool(NS, "main");
    bed.kube.remove_replica_set(NS, RS);
    bed.pass(6).await;
    let second = bed.status_of("second").unwrap();
    let accepted = second["conditions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|condition| condition["type"] == "Accepted")
        .unwrap();
    assert_eq!(accepted["status"], "True");
    assert_eq!(bed.creations().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_duplicate_resource_owns_no_replicaset_whatever_its_name_sorts_to() {
    let mut bed = bare(true);
    bed.kube
        .put_pool(NS, resource("a-newer", "uid-9", START - 10, &spec()));
    bed.kube
        .put_pool(NS, resource("z-older", "uid-8", START - 90, &spec()));
    bed.sync().await;
    let bodies = bed.creations();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["metadata"]["ownerReferences"][0]["uid"], "uid-8");
    assert_eq!(
        bodies[0]["metadata"]["ownerReferences"][0]["name"],
        "z-older"
    );
}

#[tokio::test(start_paused = true)]
async fn a_namespace_with_no_resource_makes_nothing_and_its_pods_still_expire() {
    let mut bed = bare(true);
    bed.kube.put_pod(old_warm("w0", 2000));
    bed.sync().await;
    assert!(bed.kube.pod(NS, "w0").is_none());
    assert!(bed.creations().is_empty());
}

#[tokio::test(start_paused = true)]
async fn when_the_resource_goes_the_keeper_writes_nothing_and_claimed_pods_stay() {
    let mut bed = bed(true);
    bed.kube.put_pod(PodFacts {
        idle_since: Mark::Value(at(START)),
        state_bytes: Mark::Value(50_000_000),
        ..fixtures::claimed("c0", true, START - 10)
    });
    bed.sync().await;
    bed.kube.remove_pool(NS, "main");
    bed.kube.remove_replica_set(NS, RS);
    bed.kube.clear_calls();
    bed.settle().await;
    bed.pass(30).await;
    assert!(bed.pool_writes().is_empty());
    assert!(bed.kube.calls().is_empty());
    assert!(bed.kube.pod(NS, "c0").is_some());
    assert!(!bed.has_metric("br_sandbox_keeper_pool_desired{pool=\"sbx\"} 2"));
}

#[tokio::test(start_paused = true)]
async fn a_missing_template_is_template_absent_and_never_a_fence_fault() {
    let mut bed = bare(true);
    bed.kube.remove_template(NS);
    bed.kube.put_pool(NS, main_pool());
    bed.sync().await;
    assert_eq!(
        bed.condition("Accepted"),
        Some(("False".to_owned(), "TemplateAbsent".to_owned()))
    );
    assert!(bed.creations().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_template_without_its_labels_or_its_container_is_invalid_and_makes_nothing() {
    for template in [
        json!({"metadata": {"labels": {"br.sandbox/pool": "warm"}},
               "spec": {"containers": [{"name": "agent", "image": "x"}]}}),
        json!({"metadata": {"labels": {"br.sandbox/pool": "warm", "br.sandbox/pool-id": NS}},
               "spec": {"containers": [{"name": "other", "image": "x"}]}}),
    ] {
        let mut bed = bare(true);
        bed.kube.put_template(NS, template_map(&template));
        bed.kube.put_pool(NS, main_pool());
        bed.sync().await;
        assert_eq!(
            bed.condition("Accepted"),
            Some(("False".to_owned(), "InvalidTemplate".to_owned()))
        );
        assert!(bed.creations().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn a_follower_makes_no_replicaset_and_writes_no_status() {
    let mut bed = bare(false);
    bed.kube.put_pool(NS, main_pool());
    bed.sync().await;
    bed.pass(30).await;
    assert!(bed.kube.calls().is_empty());
    assert!(bed.status().is_none());
}

#[tokio::test(start_paused = true)]
async fn the_expiry_of_pods_goes_on_for_a_resource_that_is_invalid() {
    let mut bed = bare(true);
    let mut broken = spec();
    broken.min_replicas = 30;
    bed.kube.put_pool(NS, edited(&broken, 1));
    bed.kube.put_pod(PodFacts {
        phase: Phase::Running,
        ..old_warm("w0", 2000)
    });
    bed.sync().await;
    assert!(bed.kube.pod(NS, "w0").is_none());
    assert_eq!(
        bed.condition("Accepted"),
        Some(("False".to_owned(), "InvalidSpec".to_owned()))
    );
    assert!(bed.creations().is_empty());
}
