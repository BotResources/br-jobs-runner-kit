use super::*;
use crate::cluster::fixtures::{
    AGENT, Bed, SELECTOR, binding, deny_all, managed, named, namespace, owned_replica_set, owner,
    quota, template_json, template_map,
};
use crate::pool::{FenceObject, FenceProblem, TemplateFault};

fn observer(bed: &Bed) -> Observer {
    Observer::new(bed.kube.clone(), bed.views.clone())
}

async fn fenced_bed() -> Bed {
    let bed = Bed::start();
    bed.kube.put_namespace(managed("sbx"));
    bed.kube.put_binding(binding());
    bed.kube.put_quota("sbx", quota());
    bed.kube.put_network_policies("sbx", vec![deny_all()]);
    bed.until("namespace and binding seen", |views| {
        matches!(views.namespace("sbx"), Read::Observed(Some(_)))
            && matches!(views.binding(), Read::Observed(Some(_)))
    })
    .await;
    bed
}

fn inputs(observation: FenceObservation) -> FenceInputs {
    match observation {
        FenceObservation::Observed(found) => *found,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_facts_of_a_fenced_namespace_give_the_verdict_fenced_through_the_pure_rule() {
    let bed = fenced_bed().await;
    let found = inputs(observer(&bed).fence("sbx").await);
    assert_eq!(found.quotas.len(), 1);
    assert_eq!(found.network_policies.len(), 1);
    assert!(found.binding.is_some());
    assert!(found.verdict(SELECTOR).is_fenced());
}

#[tokio::test]
async fn a_missing_quota_a_foreign_policy_and_a_missing_binding_are_read_and_judged_by_the_pure_rule()
 {
    let bed = fenced_bed().await;
    bed.kube.put_network_policies(
        "sbx",
        vec![
            deny_all(),
            named("allow-all", |metadata| {
                k8s_openapi::api::networking::v1::NetworkPolicy {
                    metadata,
                    spec: None,
                }
            }),
        ],
    );
    let verdict = inputs(observer(&bed).fence("sbx").await).verdict(SELECTOR);
    assert!(matches!(verdict, FenceVerdict::NotFenced(fault)
        if fault.object == FenceObject::NetworkPolicy && fault.problem == FenceProblem::NotInClosedList));
    let empty = Bed::start();
    empty.kube.put_namespace(managed("sbx"));
    empty.kube.put_binding(binding());
    empty.kube.put_network_policies("sbx", vec![deny_all()]);
    empty
        .until("seen", |views| {
            matches!(views.binding(), Read::Observed(Some(_)))
                && matches!(views.namespace("sbx"), Read::Observed(Some(_)))
        })
        .await;
    let verdict = inputs(observer(&empty).fence("sbx").await).verdict(SELECTOR);
    assert!(matches!(verdict, FenceVerdict::NotFenced(fault)
        if fault.object == FenceObject::ResourceQuota && fault.problem == FenceProblem::Missing));
    empty.kube.remove_binding("br-sandbox-pod");
    empty
        .until("binding gone", |views| {
            matches!(views.binding(), Read::Observed(None))
        })
        .await;
    let verdict = inputs(observer(&empty).fence("sbx").await).verdict(SELECTOR);
    assert!(matches!(verdict, FenceVerdict::NotFenced(_)));
}

#[tokio::test]
async fn a_namespace_outside_the_selector_is_not_managed_and_is_not_judged() {
    let bed = fenced_bed().await;
    bed.kube
        .put_namespace(namespace("product", &[("team", "x")]));
    assert_eq!(
        observer(&bed).fence("product").await,
        FenceObservation::NotManaged
    );
}

#[tokio::test]
async fn a_view_that_is_not_synced_gives_not_observed_and_no_read_is_made() {
    let bed = Bed::start();
    let before = bed.kube.reads().len();
    let observation = Observer::new(bed.kube.clone(), Views::new())
        .fence("sbx")
        .await;
    assert!(matches!(observation, FenceObservation::NotObserved(_)));
    assert_eq!(bed.kube.reads().len(), before);
    let bed = fenced_bed().await;
    bed.kube
        .break_watch("bindings", "", "the binding watch broke");
    bed.until("binding not synced", |views| {
        views.binding().observed().is_none()
    })
    .await;
    assert!(matches!(
        observer(&bed).fence("sbx").await,
        FenceObservation::NotObserved(reason) if reason.contains("broke")
    ));
}

#[tokio::test]
async fn a_failed_read_of_the_quota_or_the_policies_is_not_observed_never_a_default() {
    let bed = fenced_bed().await;
    for operation in ["read_quota", "list_network_policies"] {
        bed.kube.fail(operation);
        assert!(
            matches!(
                observer(&bed).fence("sbx").await,
                FenceObservation::NotObserved(_)
            ),
            "{operation}"
        );
        bed.kube.heal(operation);
    }
    assert!(matches!(
        observer(&bed).fence("sbx").await,
        FenceObservation::Observed(_)
    ));
}

#[tokio::test]
async fn the_template_is_read_from_the_one_config_map_and_judged_by_the_pure_rule() {
    let bed = Bed::start();
    let keys = br_runner_sandbox::PodKeys::default();
    let absent = observer(&bed).template("sbx", &keys, AGENT).await;
    assert!(matches!(absent, Read::Observed(Err(TemplateFault::Absent))));
    bed.kube
        .put_template("sbx", template_map(&template_json("sbx")));
    let found = observer(&bed).template("sbx", &keys, AGENT).await;
    assert!(matches!(found, Read::Observed(Ok(_))));
    bed.kube
        .put_template("sbx", template_map(&template_json("another")));
    let invalid = observer(&bed).template("sbx", &keys, AGENT).await;
    assert!(matches!(
        invalid,
        Read::Observed(Err(TemplateFault::Invalid(_)))
    ));
}

#[tokio::test]
async fn a_failed_read_of_the_template_is_not_observed_and_not_template_absent() {
    let bed = Bed::start();
    bed.kube.fail("read_template");
    let keys = br_runner_sandbox::PodKeys::default();
    let found = observer(&bed).template("sbx", &keys, AGENT).await;
    assert!(matches!(found, Read::NotObserved(reason) if reason.contains("pod template")));
}

#[tokio::test]
async fn the_replica_set_is_absent_owned_or_not_owned_and_a_failed_read_is_not_observed() {
    let bed = Bed::start();
    let read = observer(&bed);
    assert_eq!(
        read.replica_set("sbx", &owner()).await,
        Read::Observed(ReplicaSetRead::Absent)
    );
    bed.kube.put_replica_set(owned_replica_set("sbx", 3));
    assert!(matches!(
        read.replica_set("sbx", &owner()).await,
        Read::Observed(ReplicaSetRead::Owned(found)) if found.metadata.uid.as_deref() == Some("rs-uid")
    ));
    let other = crate::pool::Owner::new("main", "uid-2");
    assert_eq!(
        read.replica_set("sbx", &other).await,
        Read::Observed(ReplicaSetRead::NotOwned(
            OwnershipFault::NotControlledByResource
        ))
    );
    bed.kube.fail("read_replica_set");
    assert!(matches!(
        read.replica_set("sbx", &owner()).await,
        Read::NotObserved(_)
    ));
}
