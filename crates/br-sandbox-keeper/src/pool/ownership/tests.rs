use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};

use super::*;

type Change = fn(&mut OwnerReference);

fn reference(uid: &str) -> OwnerReference {
    owner_reference(&Owner::new("main", uid))
}

fn replica_set(change: impl FnOnce(&mut ObjectMeta)) -> ReplicaSet {
    let mut metadata = ObjectMeta {
        name: Some("warm-sbx".to_owned()),
        namespace: Some("sbx".to_owned()),
        owner_references: Some(vec![reference("uid-1")]),
        ..ObjectMeta::default()
    };
    change(&mut metadata);
    ReplicaSet {
        metadata,
        ..ReplicaSet::default()
    }
}

fn check(replica_set: &ReplicaSet) -> Result<(), OwnershipFault> {
    check_ownership(replica_set, "sbx", &Owner::new("main", "uid-1"))
}

#[test]
fn a_replica_set_of_the_namespace_with_the_name_and_the_controller_reference_is_owned() {
    assert_eq!(check(&replica_set(|_| {})), Ok(()));
}

#[test]
fn the_version_of_the_owner_reference_does_not_count() {
    let other_version = replica_set(|meta| {
        let mut changed = reference("uid-1");
        changed.api_version = "sandbox.botresources.ai/v1beta7".to_owned();
        meta.owner_references = Some(vec![changed]);
    });
    assert_eq!(check(&other_version), Ok(()));
}

#[test]
fn a_replica_set_of_another_namespace_or_name_or_in_deletion_is_not_owned() {
    assert_eq!(
        check(&replica_set(
            |meta| meta.namespace = Some("other".to_owned())
        )),
        Err(OwnershipFault::OtherNamespace)
    );
    assert_eq!(
        check(&replica_set(|meta| meta.namespace = None)),
        Err(OwnershipFault::OtherNamespace)
    );
    assert_eq!(
        check(&replica_set(
            |meta| meta.name = Some("warm-other".to_owned())
        )),
        Err(OwnershipFault::OtherName)
    );
    let now = Time(k8s_openapi::jiff::Timestamp::UNIX_EPOCH);
    assert_eq!(
        check(&replica_set(|meta| meta.deletion_timestamp = Some(now))),
        Err(OwnershipFault::Deleting)
    );
}

#[test]
fn an_owner_reference_that_is_not_the_controller_of_this_resource_does_not_give_ownership() {
    let cases: [(&str, Change); 6] = [
        ("another uid", |r| r.uid = "uid-2".to_owned()),
        ("not a controller", |r| r.controller = Some(false)),
        ("no controller flag", |r| r.controller = None),
        ("another kind", |r| r.kind = "Deployment".to_owned()),
        ("another group", |r| {
            r.api_version = "apps/v1alpha1".to_owned()
        }),
        ("no group", |r| r.api_version = "v1".to_owned()),
    ];
    for (name, change) in cases {
        let found = replica_set(|meta| {
            let mut changed = reference("uid-1");
            change(&mut changed);
            meta.owner_references = Some(vec![changed]);
        });
        assert_eq!(
            check(&found),
            Err(OwnershipFault::NotControlledByResource),
            "{name}"
        );
    }
    let none = replica_set(|meta| meta.owner_references = None);
    assert_eq!(check(&none), Err(OwnershipFault::NotControlledByResource));
}

#[test]
fn the_owner_of_a_resource_needs_a_name_and_a_uid() {
    let mut pool = SandboxPool {
        metadata: ObjectMeta::default(),
        spec: serde_json::Value::Null,
        status: None,
    };
    assert_eq!(Owner::of(&pool), None);
    pool.metadata.name = Some("main".to_owned());
    assert_eq!(Owner::of(&pool), None);
    pool.metadata.uid = Some("uid-1".to_owned());
    assert_eq!(Owner::of(&pool), Some(Owner::new("main", "uid-1")));
}
