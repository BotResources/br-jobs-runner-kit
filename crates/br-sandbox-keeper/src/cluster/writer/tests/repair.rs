use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ManagedFieldsEntry;
use serde_json::json;

use super::whole::{all_size_edits, made, template_of};
use super::*;
use crate::cluster::FIELD_MANAGER;
use crate::pool::{replica_set_body, template_hash_key};

fn template_writes(rig: &Rig) -> usize {
    rig.kube
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::ApplyTemplate { .. }))
        .count()
}

fn holds_template(set: &ReplicaSet, manager: &str) -> bool {
    set.metadata
        .managed_fields
        .iter()
        .flatten()
        .filter(|entry| entry.manager.as_deref() == Some(manager))
        .filter(|entry| entry.operation.as_deref() == Some("Apply"))
        .any(|entry| {
            entry
                .fields_v1
                .as_ref()
                .is_some_and(|fields| fields.0["f:spec"].get("f:template").is_some())
        })
}

fn with_hash_restored(rig: &Rig, current: &RenderedTemplate) {
    let mut set = rig.kube.raw_replica_set("sbx", "warm-sbx").unwrap();
    let key = template_hash_key(PodKeys::default().prefix());
    let template = set
        .spec
        .as_mut()
        .and_then(|spec| spec.template.as_mut())
        .unwrap();
    template
        .metadata
        .get_or_insert_with(Default::default)
        .annotations
        .get_or_insert_with(Default::default)
        .insert(key, current.hash.clone());
    rig.kube.put_replica_set(set);
}

fn applied_by_the_previous_version(rig: &Rig, next: &RenderedTemplate, replicas: i32) {
    rig.kube
        .apply_as(
            FIELD_MANAGER,
            "sbx",
            "warm-sbx",
            &json!({"spec": {"template": next.template}}),
        )
        .unwrap();
    rig.kube
        .apply_as(
            FIELD_MANAGER,
            "sbx",
            "warm-sbx",
            &json!({"spec": {"replicas": replicas}}),
        )
        .unwrap();
}

async fn repaired_by_one_write(rig: &Rig, wanted: &RenderedTemplate) {
    assert_eq!(
        rig.writer.set_template("sbx", &owner(), wanted).await,
        Ok(Change::Written)
    );
    assert_eq!(template_writes(rig), 1);
    assert_eq!(template_of(rig), wanted.template);
    assert_eq!(
        rig.writer.set_template("sbx", &owner(), wanted).await,
        Ok(Change::Unchanged)
    );
    for replicas in [6, 7, 2] {
        assert_eq!(
            rig.writer.set_replicas("sbx", &owner(), replicas).await,
            Ok(Change::Written)
        );
        assert_eq!(template_of(rig), wanted.template);
        assert_eq!(
            rig.writer.set_template("sbx", &owner(), wanted).await,
            Ok(Change::Unchanged)
        );
    }
    assert_eq!(template_writes(rig), 1);
}

#[tokio::test]
async fn a_replica_set_that_lost_template_fields_to_a_replicas_apply_is_repaired_by_one_write_while_its_hash_is_current()
 {
    let rig = rig();
    let base = rendered_with(&settings(), &template_json("sbx"));
    let edited = rendered_with(&all_size_edits(), &template_json("sbx"));
    made(&rig, &base).await;
    applied_by_the_previous_version(&rig, &edited, 4);
    assert_ne!(template_of(&rig), edited.template);
    with_hash_restored(&rig, &edited);
    let damaged = rig.kube.raw_replica_set("sbx", "warm-sbx").unwrap();
    assert!(!holds_template(&damaged, FIELD_MANAGER));
    assert!(!holds_template(
        &damaged,
        crate::cluster::TEMPLATE_FIELD_MANAGER
    ));
    let key = template_hash_key(PodKeys::default().prefix());
    assert_eq!(
        damaged
            .spec
            .unwrap()
            .template
            .unwrap()
            .metadata
            .unwrap()
            .annotations
            .unwrap()[&key],
        edited.hash
    );
    repaired_by_one_write(&rig, &edited).await;
}

#[tokio::test]
async fn a_replica_set_that_lost_template_fields_and_its_hash_is_repaired_by_one_write() {
    let rig = rig();
    let base = rendered_with(&settings(), &template_json("sbx"));
    let edited = rendered_with(&all_size_edits(), &template_json("sbx"));
    made(&rig, &base).await;
    applied_by_the_previous_version(&rig, &edited, 4);
    assert_ne!(template_of(&rig), edited.template);
    repaired_by_one_write(&rig, &edited).await;
}

#[tokio::test]
async fn a_replica_set_whose_template_the_previous_version_applied_and_kept_is_repaired_by_one_write()
 {
    let rig = rig();
    let base = rendered_with(&settings(), &template_json("sbx"));
    let edited = rendered_with(&all_size_edits(), &template_json("sbx"));
    made(&rig, &base).await;
    rig.kube
        .apply_as(
            FIELD_MANAGER,
            "sbx",
            "warm-sbx",
            &json!({"spec": {"template": edited.template}}),
        )
        .unwrap();
    assert!(holds_template(
        &rig.kube.raw_replica_set("sbx", "warm-sbx").unwrap(),
        FIELD_MANAGER
    ));
    assert_eq!(template_of(&rig), edited.template);
    repaired_by_one_write(&rig, &edited).await;
}

#[tokio::test]
async fn a_replica_set_that_the_keeper_made_gets_one_template_write_and_then_none() {
    let rig = rig();
    let first = rendered("img:1");
    made(&rig, &first).await;
    assert_eq!(template_of(&rig), first.template);
    repaired_by_one_write(&rig, &first).await;
}

fn put_with_managed_fields(
    rig: &Rig,
    rendered: &RenderedTemplate,
    managed: Option<Vec<ManagedFieldsEntry>>,
) {
    let body = replica_set_body("sbx", &owner(), 2, rendered, &PodKeys::default());
    let mut set: ReplicaSet = serde_json::from_value(body).unwrap();
    set.metadata.uid = Some("rs-uid".to_owned());
    set.metadata.managed_fields = managed;
    rig.kube.put_replica_set(set);
}

#[tokio::test]
async fn a_replica_set_read_without_managed_fields_is_judged_by_its_hash_and_never_written_in_a_loop()
 {
    for managed in [None, Some(Vec::new())] {
        let rig = rig();
        let wanted = rendered("img:1");
        put_with_managed_fields(&rig, &wanted, managed);
        for _ in 0..5 {
            assert_eq!(
                rig.writer.set_template("sbx", &owner(), &wanted).await,
                Ok(Change::Unchanged)
            );
        }
        assert!(rig.kube.calls().is_empty());
        let changed = rendered("img:2");
        assert_eq!(
            rig.writer.set_template("sbx", &owner(), &changed).await,
            Ok(Change::Written)
        );
        assert_eq!(template_of(&rig), changed.template);
    }
}
