use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

use crate::resource::{PoolSize, SandboxPool, SandboxPoolSpec, SizeBounds};

use super::*;

fn pool_of_main() -> SandboxPool {
    SandboxPool::new(
        ObjectMeta {
            name: Some("main".to_owned()),
            uid: Some("uid-1".to_owned()),
            generation: Some(1),
            ..ObjectMeta::default()
        },
        &SandboxPoolSpec::new(
            "registry.example/agent:1",
            PoolSize::new(
                SizeBounds::new("500m", "2"),
                SizeBounds::new("1Gi", "4Gi"),
                SizeBounds::new("1Gi", "8Gi"),
            ),
            1,
            20,
        ),
    )
    .unwrap()
}

#[tokio::test(start_paused = true)]
async fn the_keeper_makes_the_pool_of_a_resource_and_writes_its_status() {
    let rig = rig();
    rig.kube.put_binding(crate::cluster::fixtures::binding());
    rig.kube.put_pool("sbx", pool_of_main());
    let (listener, address) = listener().await;
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(run_on(
        listener,
        testing::config(),
        parts(&rig, "a"),
        shutdown.clone(),
    ));
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert_eq!(request(address, "GET", "/readyz").await.0, 200);
    assert!(rig.kube.raw_replica_set("sbx", "warm-sbx").is_some());
    let status = rig.kube.pool_status("sbx", "main").unwrap();
    assert!(
        status["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|condition| condition["type"] == "Accepted" && condition["status"] == "True")
    );
    shutdown.cancel();
    assert_eq!(handle.await.unwrap(), Ok(()));
}

#[tokio::test(start_paused = true)]
async fn an_absent_definition_of_the_resource_is_not_ready_loud_and_writes_no_replicaset_while_pods_expire()
 {
    let rig = rig();
    rig.kube.put_binding(crate::cluster::fixtures::binding());
    rig.kube.remove_definition("sbx");
    rig.kube.put_pod(fixtures::warm("w0", START - 5000));
    let (listener, address) = listener().await;
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(run_on(
        listener,
        testing::config(),
        parts(&rig, "a"),
        shutdown.clone(),
    ));
    tokio::time::sleep(Duration::from_secs(10)).await;
    let (status, _, body) = request(address, "GET", "/readyz").await;
    assert_eq!(status, 503);
    assert!(body.contains("absent"), "{body}");
    assert!(rig.kube.raw_replica_set("sbx", "warm-sbx").is_none());
    assert!(rig.kube.pod("sbx", "w0").is_none());
    shutdown.cancel();
    assert_eq!(handle.await.unwrap(), Ok(()));
}
