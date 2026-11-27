use crate::facts::{ConditionFacts, Mark, Phase, PodFacts, fixtures};

use super::*;

const OTHER: &str = "other";

fn ghost(pod: PodFacts) -> PodFacts {
    PodFacts {
        namespace: OTHER.to_owned(),
        pool_id: Some("ghost".to_owned()),
        ..pod
    }
}

fn failing_condition() -> ConditionFacts {
    ConditionFacts {
        kind: "WarmPodsFailing".to_owned(),
        status: "True".to_owned(),
        reason: Some("ImagePullFailing".to_owned()),
        message: Some("1 warm pod(s) failing; kubelet reason: ErrImagePull".to_owned()),
        last_transition_time: Some(at(START)),
    }
}

fn pulling(name: &str) -> PodFacts {
    PodFacts {
        phase: Phase::Pending,
        ready: false,
        containers: vec![fixtures::waiting("ErrImagePull")],
        ..old_warm(name, 10)
    }
}

#[tokio::test(start_paused = true)]
async fn a_new_leader_judges_no_pool_before_the_first_sync_of_its_namespace() {
    let mut bed = bed(false);
    bed.kube.put_pod(pulling("a"));
    bed.kube
        .set_replica_set_conditions(NS, RS, vec![failing_condition()]);
    bed.leadership.renewed(at(START + 1_000_000));
    bed.engine.gained_leadership().await;
    bed.engine.wake().await;
    assert!(bed.kube.calls().is_empty());
    bed.sync().await;
    assert!(bed.kube.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_pod_that_has_been_ready_and_then_crashes_does_not_make_the_pool_failing() {
    let mut bed = bed(true);
    bed.sync().await;
    let ready = bed.kube.put_pod(old_warm("w0", 10));
    bed.applied(ready.clone()).await;
    bed.kube.clear_calls();
    bed.applied(PodFacts {
        ready: false,
        containers: vec![fixtures::waiting("CrashLoopBackOff")],
        ..ready
    })
    .await;
    bed.pass(30).await;
    assert!(
        !bed.kube
            .calls()
            .iter()
            .any(|call| matches!(call, Call::Condition { status, .. } if status == "True"))
    );
}

#[tokio::test(start_paused = true)]
async fn the_pods_of_a_pool_that_is_not_configured_expire_with_the_default_pool_values() {
    let mut bed = bed(true);
    let mut document = testing::document();
    document["pools"][0]["idle_ttl_seconds"] = 600.into();
    bed.engine.config = Arc::new(testing::config_of(&document));
    bed.sync().await;
    let keep_big = |name: &str, pod: PodFacts| PodFacts {
        idle_since: Mark::Value(at(START)),
        state_bytes: Mark::Value(20_000_000),
        ..pod_named(name, pod)
    };
    for pod in [
        ghost(old_warm("g-warm", 1700)),
        ghost(PodFacts {
            idle_since: Mark::Value(at(START)),
            state_bytes: Mark::Value(5),
            ..fixtures::claimed("g-small", true, START)
        }),
        ghost(keep_big("g-big", fixtures::claimed("x", true, START))),
        ghost(fixtures::claimed("g-dirty", false, START - 7000)),
        ghost(PodFacts {
            pool_id: None,
            ..old_warm("g-nopool", 1700)
        }),
        keep_big("p-big", fixtures::claimed("x", true, START)),
    ] {
        bed.kube.put_pod(pod);
    }
    for namespace in [NS, OTHER] {
        let pods = bed.kube.pods_in(namespace);
        bed.engine.handle(namespace, PodEvent::Snapshot(pods)).await;
    }
    assert!(bed.kube.pod(OTHER, "g-small").is_none());
    for name in ["g-warm", "g-big", "g-dirty", "g-nopool"] {
        assert!(bed.kube.pod(OTHER, name).is_some(), "{name}");
    }
    bed.pass(101).await;
    assert!(bed.kube.pod(OTHER, "g-warm").is_none());
    assert!(bed.kube.pod(OTHER, "g-nopool").is_none());
    assert!(bed.kube.pod(OTHER, "g-big").is_some());
    bed.pass(20).await;
    assert!(bed.kube.pod(OTHER, "g-big").is_none());
    assert!(bed.kube.pod(NS, "p-big").is_some());
    assert!(bed.kube.pod(OTHER, "g-dirty").is_some());
    bed.pass(80).await;
    assert!(bed.kube.pod(OTHER, "g-dirty").is_none());
    bed.pass(400).await;
    assert!(bed.kube.pod(NS, "p-big").is_none());
    assert!(bed.has_metric("br_sandbox_keeper_pods_deleted_total{reason=\"idle\"} 3"));
    for call in bed.kube.calls() {
        match call {
            Call::Delete { .. } => {}
            Call::Condition { name, .. } | Call::Replicas { name, .. } => assert_eq!(name, RS),
            other => panic!("a call outside the configured pool: {other:?}"),
        }
    }
}

fn pod_named(name: &str, pod: PodFacts) -> PodFacts {
    PodFacts {
        name: name.to_owned(),
        uid: format!("uid-{name}"),
        ..pod
    }
}
