use super::*;
use crate::cluster::fixtures::{owned_replica_set, owner};
use crate::fakes::Call;
use crate::rules::ConditionWrite;
use crate::time::at;

fn write() -> ConditionWrite {
    ConditionWrite {
        status: "True",
        reason: "ImagePullFailing",
        message: "1 warm pod(s) failing; kubelet reason: ErrImagePull".to_owned(),
        last_transition_time: at(1),
    }
}

#[tokio::test]
async fn the_condition_is_written_on_the_status_of_an_owned_replicaset() {
    let rig = rig();
    rig.kube.put_replica_set(owned_replica_set("sbx", 3));
    assert_eq!(
        rig.writer.set_condition("sbx", &owner(), &write()).await,
        Ok(())
    );
    assert_eq!(
        rig.kube.calls(),
        [Call::Condition {
            name: "warm-sbx".to_owned(),
            status: "True".to_owned(),
            reason: "ImagePullFailing".to_owned(),
            message: "1 warm pod(s) failing; kubelet reason: ErrImagePull".to_owned(),
        }]
    );
}

#[tokio::test]
async fn the_condition_is_never_written_on_a_replicaset_that_the_resource_does_not_own() {
    let rig = rig();
    let mut foreign = owned_replica_set("sbx", 3);
    foreign.metadata.owner_references = None;
    rig.kube.put_replica_set(foreign);
    assert!(matches!(
        rig.writer.set_condition("sbx", &owner(), &write()).await,
        Err(WriteError::NotOwned(_))
    ));
    assert!(rig.kube.calls().is_empty());
}

#[tokio::test]
async fn the_condition_of_a_replicaset_that_is_gone_is_a_typed_error_and_writes_nothing() {
    let rig = rig();
    assert_eq!(
        rig.writer.set_condition("sbx", &owner(), &write()).await,
        Err(WriteError::Gone {
            call: "apply_condition"
        })
    );
    assert!(rig.kube.calls().is_empty());
}

#[tokio::test]
async fn a_refused_write_of_the_condition_is_a_refused_error_and_is_counted() {
    let rig = rig();
    rig.kube.put_replica_set(owned_replica_set("sbx", 3));
    rig.kube.forbid("apply_condition");
    assert!(matches!(
        rig.writer.set_condition("sbx", &owner(), &write()).await,
        Err(WriteError::Refused { .. })
    ));
    assert!(errors(&rig));
}
