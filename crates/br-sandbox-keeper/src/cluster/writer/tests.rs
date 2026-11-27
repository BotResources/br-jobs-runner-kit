use super::*;
use crate::cluster::fixtures::{AGENT, owned_replica_set, owner, template_json, template_map};
use crate::fakes::{Call, FakeKube};
use crate::pool::PoolTemplate;
use crate::testing::settings;

pub(super) struct Rig {
    pub(super) kube: Arc<FakeKube>,
    pub(super) metrics: Arc<Metrics>,
    pub(super) writer: PoolWriter,
}

pub(super) fn rig() -> Rig {
    let kube = Arc::new(FakeKube::default());
    let metrics = Arc::new(Metrics::new());
    let writer = PoolWriter::new(kube.clone(), metrics.clone(), PodKeys::default());
    Rig {
        kube,
        metrics,
        writer,
    }
}

pub(super) fn rendered(image: &str) -> RenderedTemplate {
    let keys = PodKeys::default();
    let map = template_map(&template_json("sbx"));
    let template = PoolTemplate::read(Some(&map), "sbx", &keys, AGENT).unwrap();
    let mut wanted = settings();
    wanted.image = image.to_owned();
    template.render(&wanted, &keys)
}

pub(super) fn errors(rig: &Rig) -> bool {
    rig.metrics
        .render(true)
        .contains("br_sandbox_keeper_sweep_errors_total 1")
}

mod condition;
mod more;

#[tokio::test]
async fn the_replica_set_is_created_with_the_controller_reference_and_no_block_owner_deletion() {
    let rig = rig();
    let outcome = rig
        .writer
        .create_replica_set("sbx", &owner(), 4, &rendered("img:1"))
        .await;
    assert_eq!(outcome, Ok(CreateOutcome::Created));
    let created = rig.kube.raw_replica_set("sbx", "warm-sbx").unwrap();
    let reference = &created.metadata.owner_references.clone().unwrap()[0];
    assert_eq!(reference.controller, Some(true));
    assert_eq!(reference.block_owner_deletion, None);
    assert_eq!(reference.uid, "uid-1");
    assert!(created.metadata.finalizers.is_none());
    assert_eq!(created.spec.as_ref().unwrap().replicas, Some(4));
    let calls = rig.kube.calls();
    assert_eq!(calls.len(), 1);
    assert!(matches!(&calls[0], Call::CreateReplicaSet { name, body }
        if name == "warm-sbx" && body["metadata"]["ownerReferences"][0].get("blockOwnerDeletion").is_none()));
}

#[tokio::test]
async fn a_create_that_finds_the_name_taken_answers_exists_and_changes_nothing() {
    let rig = rig();
    rig.kube.put_replica_set(owned_replica_set("sbx", 9));
    let outcome = rig
        .writer
        .create_replica_set("sbx", &owner(), 1, &rendered("img:1"))
        .await;
    assert_eq!(outcome, Ok(CreateOutcome::Exists));
    let kept = rig.kube.raw_replica_set("sbx", "warm-sbx").unwrap();
    assert_eq!(kept.spec.unwrap().replicas, Some(9));
}

#[tokio::test]
async fn the_replicas_are_written_only_when_the_value_differs() {
    let rig = rig();
    rig.kube.put_replica_set(owned_replica_set("sbx", 3));
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), 3).await,
        Ok(Change::Unchanged)
    );
    assert!(rig.kube.calls().is_empty());
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), 5).await,
        Ok(Change::Written)
    );
    assert_eq!(
        rig.kube.calls(),
        [Call::Replicas {
            name: "warm-sbx".to_owned(),
            replicas: 5
        }]
    );
    assert_eq!(
        rig.kube
            .raw_replica_set("sbx", "warm-sbx")
            .and_then(|set| set.spec)
            .and_then(|spec| spec.replicas),
        Some(5)
    );
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), 5).await,
        Ok(Change::Unchanged)
    );
    assert_eq!(rig.kube.calls().len(), 1);
}

#[tokio::test]
async fn a_replica_set_that_the_resource_does_not_own_is_never_written_or_deleted() {
    let rig = rig();
    let mut foreign = owned_replica_set("sbx", 3);
    foreign.metadata.owner_references = None;
    rig.kube.put_replica_set(foreign);
    let other = Owner::new("main", "uid-1");
    let refused = Err(WriteError::NotOwned(
        OwnershipFault::NotControlledByResource,
    ));
    assert_eq!(rig.writer.set_replicas("sbx", &other, 9).await, refused);
    assert_eq!(
        rig.writer
            .set_template("sbx", &other, &rendered("img:1"))
            .await,
        refused
    );
    assert_eq!(
        rig.writer
            .delete_replica_set("sbx", &other)
            .await
            .map(|_| ()),
        refused.map(|_: Change| ())
    );
    assert!(rig.kube.calls().is_empty());
    assert!(rig.kube.raw_replica_set("sbx", "warm-sbx").is_some());
}

#[tokio::test]
async fn a_replica_set_in_deletion_or_owned_by_another_uid_is_not_owned() {
    let rig = rig();
    rig.kube.put_replica_set(owned_replica_set("sbx", 3));
    let stranger = Owner::new("main", "uid-2");
    assert!(matches!(
        rig.writer.set_replicas("sbx", &stranger, 9).await,
        Err(WriteError::NotOwned(
            OwnershipFault::NotControlledByResource
        ))
    ));
    let mut dying = owned_replica_set("sbx", 3);
    dying.metadata.deletion_timestamp = Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
        k8s_openapi::jiff::Timestamp::UNIX_EPOCH,
    ));
    rig.kube.put_replica_set(dying);
    assert!(matches!(
        rig.writer.delete_replica_set("sbx", &owner()).await,
        Err(WriteError::NotOwned(OwnershipFault::Deleting))
    ));
    assert!(rig.kube.calls().is_empty());
}

#[tokio::test]
async fn an_absent_replica_set_is_gone_for_a_resize_and_nothing_to_delete() {
    let rig = rig();
    assert_eq!(
        rig.writer.set_replicas("sbx", &owner(), 1).await,
        Err(WriteError::Gone {
            call: "apply_replicas"
        })
    );
    assert_eq!(
        rig.writer.delete_replica_set("sbx", &owner()).await,
        Ok(DeleteOutcome::Gone)
    );
    assert!(rig.kube.calls().is_empty());
}

#[tokio::test]
async fn the_template_is_written_only_when_its_hash_changed_and_the_write_is_the_template_alone() {
    let rig = rig();
    rig.kube.put_replica_set(owned_replica_set("sbx", 3));
    let first = rendered("img:1");
    assert_eq!(
        rig.writer.set_template("sbx", &owner(), &first).await,
        Ok(Change::Written)
    );
    assert_eq!(
        rig.kube.calls(),
        [Call::ApplyTemplate {
            name: "warm-sbx".to_owned(),
            template: first.template.clone()
        }]
    );
    let held = rig.kube.raw_replica_set("sbx", "warm-sbx").unwrap();
    let annotations = held
        .spec
        .unwrap()
        .template
        .unwrap()
        .metadata
        .unwrap()
        .annotations
        .unwrap();
    assert_eq!(
        annotations.get("br.sandbox/template-hash"),
        Some(&first.hash)
    );
    assert_eq!(
        rig.writer.set_template("sbx", &owner(), &first).await,
        Ok(Change::Unchanged)
    );
    assert_eq!(rig.kube.calls().len(), 1);
    let second = rendered("img:2");
    assert_ne!(first.hash, second.hash);
    assert_eq!(
        rig.writer.set_template("sbx", &owner(), &second).await,
        Ok(Change::Written)
    );
    assert_eq!(rig.kube.calls().len(), 2);
}
