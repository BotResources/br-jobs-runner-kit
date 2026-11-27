use serde_json::json;

use super::*;
use crate::cluster::fixtures::{owned_replica_set, owner};
use crate::facts::{PodFacts, PoolState};
use crate::fakes::Call;
use crate::pool::replicaset_name;
use crate::resource::SandboxPoolStatus;
use crate::time::at;

#[tokio::test]
async fn an_owned_replica_set_is_deleted_with_its_uid_as_precondition() {
    let rig = rig();
    rig.kube.put_replica_set(owned_replica_set("sbx", 3));
    assert_eq!(
        rig.writer.delete_replica_set("sbx", &owner()).await,
        Ok(DeleteOutcome::Deleted)
    );
    assert_eq!(
        rig.kube.calls(),
        [Call::DeleteReplicaSet {
            name: replicaset_name("sbx"),
            uid: "rs-uid".to_owned()
        }]
    );
    assert!(rig.kube.raw_replica_set("sbx", "warm-sbx").is_none());
}

fn warm_pod() -> PodFacts {
    PodFacts::new("sbx", "w0", "uid-w0", "1", PoolState::Warm, at(1))
}

#[tokio::test]
async fn a_pod_is_deleted_with_the_uid_and_the_resource_version_and_a_stale_version_is_a_conflict()
{
    let rig = rig();
    let pod = rig.kube.put_pod(warm_pod());
    let stale = PodVersion::new("uid-w0", "0");
    assert_eq!(
        rig.writer.delete_pod("sbx", "w0", &stale).await,
        Ok(DeleteOutcome::Conflict)
    );
    assert!(rig.kube.pod("sbx", "w0").is_some());
    assert_eq!(
        rig.writer.delete_pod("sbx", "w0", &pod.version()).await,
        Ok(DeleteOutcome::Deleted)
    );
    assert_eq!(
        rig.writer.delete_pod("sbx", "w0", &pod.version()).await,
        Ok(DeleteOutcome::Gone)
    );
    assert_eq!(
        rig.kube.calls()[0],
        Call::Delete {
            name: "w0".to_owned(),
            uid: "uid-w0".to_owned(),
            resource_version: "0".to_owned()
        }
    );
}

#[tokio::test]
async fn the_status_is_patched_on_the_status_subresource_only() {
    let rig = rig();
    let status = SandboxPoolStatus {
        desired_replicas: Some(4),
        ..SandboxPoolStatus::default()
    };
    rig.writer
        .patch_status("sbx", "main", &status)
        .await
        .unwrap();
    assert_eq!(
        rig.kube.pool_status("sbx", "main"),
        Some(json!({"desiredReplicas": 4}))
    );
    assert_eq!(rig.kube.calls().len(), 1);
    assert!(rig.kube.reads().is_empty());
}

#[tokio::test]
async fn a_refused_call_is_logged_counted_and_returned_as_a_typed_error_and_the_caller_may_retry() {
    let rig = rig();
    rig.kube.put_replica_set(owned_replica_set("sbx", 3));
    rig.kube.forbid("delete_replica_set");
    let refused = rig.writer.delete_replica_set("sbx", &owner()).await;
    assert!(matches!(
        refused,
        Err(WriteError::Refused {
            call: "delete_replica_set",
            ..
        })
    ));
    assert!(errors(&rig));
    assert!(rig.kube.raw_replica_set("sbx", "warm-sbx").is_some());
    rig.kube.permit("delete_replica_set");
    assert_eq!(
        rig.writer.delete_replica_set("sbx", &owner()).await,
        Ok(DeleteOutcome::Deleted)
    );
}

#[tokio::test]
async fn every_write_that_the_server_refuses_gives_the_refused_error() {
    let rig = rig();
    rig.kube.put_replica_set(owned_replica_set("sbx", 3));
    let pod = rig.kube.put_pod(warm_pod());
    for call in [
        "create_replica_set",
        "apply_replicas",
        "apply_template",
        "delete_pod",
        "patch_pool_status",
    ] {
        rig.kube.forbid(call);
    }
    let status = SandboxPoolStatus::default();
    let results = [
        rig.writer
            .create_replica_set("sbx", &owner(), 1, &rendered("i"))
            .await
            .map(|_| ()),
        rig.writer
            .set_replicas("sbx", &owner(), 9)
            .await
            .map(|_| ()),
        rig.writer
            .set_template("sbx", &owner(), &rendered("i"))
            .await
            .map(|_| ()),
        rig.writer
            .delete_pod("sbx", "w0", &pod.version())
            .await
            .map(|_| ()),
        rig.writer.patch_status("sbx", "main", &status).await,
    ];
    for result in results {
        assert!(
            matches!(result, Err(WriteError::Refused { .. })),
            "{result:?}"
        );
    }
    assert!(rig.kube.pod("sbx", "w0").is_some());
    assert_eq!(rig.kube.replica_set("sbx", "warm-sbx").replicas, Some(3));
    rig.kube.clear_reads();
    assert!(rig.kube.reads().is_empty());
}

#[tokio::test]
async fn a_failed_read_before_a_write_writes_nothing_and_is_counted() {
    let rig = rig();
    rig.kube.put_replica_set(owned_replica_set("sbx", 3));
    rig.kube.fail("read_replica_set");
    let result = rig.writer.set_replicas("sbx", &owner(), 9).await;
    assert!(matches!(result, Err(WriteError::Failed { .. })));
    assert!(rig.kube.calls().is_empty());
    assert!(errors(&rig));
}
