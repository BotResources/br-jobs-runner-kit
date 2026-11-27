use br_agentic_harness_contract::{AlreadySaved, PortError};
use br_runner_sandbox::{Error, ExecError, RecordError, manifest_key};

use crate::fakes::{Ids, POD, Rig, Script, rig};
use crate::flow_support::{failed_with, record_of};
use crate::part_tar::sample_tar;

const KILLED: i32 = 137;

async fn assert_killed_save_is_a_fault_without_a_pod_read(rig: Rig) {
    let ids = Ids::new();
    let key = ids.key(&rig);
    rig.cluster.push_script(Script {
        stdout: sample_tar(),
        code: Some(KILLED),
        ..Script::default()
    });
    let error = rig.env.save(POD, &key, None).await.expect_err("a fault");
    assert!(matches!(
        error,
        Error::Exec(ExecError::Failed { code: KILLED, .. })
    ));
    let port = PortError::from(error);
    assert!(!port.is_refusal());
    assert!(!AlreadySaved::is(&port));
    assert_eq!(failed_with(&record_of(&rig, &key)), RecordError::Exec);
    assert!(rig.store.object(&manifest_key(&key)).is_none());
    assert!(rig.cluster.patches().is_empty());
    assert_eq!(rig.cluster.pod_reads(), 0);
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_save_with_a_status_of_137_in_a_pod_that_is_gone_is_a_fault_and_reads_no_pod()
 {
    let rig = rig();
    rig.cluster.remove_pod(POD);
    assert_killed_save_is_a_fault_without_a_pod_read(rig).await;
}

#[tokio::test(start_paused = true)]
async fn fake_cluster_and_store_a_save_with_a_status_of_137_in_a_running_pod_is_a_fault_and_reads_no_pod()
 {
    let rig = rig();
    rig.cluster.set_phase(POD, Some("Running"));
    assert_killed_save_is_a_fault_without_a_pod_read(rig).await;
}
