use std::time::Duration;

use battery::Session;
use battery::adapter::{self, Tunables};
use battery::common::{empty_pool, scale_pool, wait_until};
use battery::error::Res;
use battery::evidence::Checks;
use battery::guards::ScaleGuard;
use battery::ids::Lane;
use battery::pool;
use br_agentic_harness_contract::Seed;
use br_runner_sandbox::{ANNOTATION_CLEAN, CLEAN_FALSE, LABEL_POOL, LABEL_ROLE, POOL_CLAIMED};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_opens_that_race_for_one_warm_pod_give_it_to_one_only() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let keeper = ScaleGuard::scale_keeper(env, 0).await?;
    let mut checks = Checks::new(env);
    for round in 0..3 {
        scale_pool(env, 1).await?;
        wait_until("exactly one warm pod", 120, || async {
            Ok(pool::warm_list(env, true).await?.len() == 1)
        })
        .await?;
        let left = Tunables::standard(env).connect(env).await?;
        let right = Tunables::standard(env).connect(env).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
        let (a, b) = (Lane::new()?.first()?.1, Lane::new()?.first()?.1);
        let (one, two) = tokio::join!(
            adapter::open(&left, a, Seed::empty()),
            adapter::open(&right, b, Seed::empty())
        );
        let (one, two) = (one?, two?);
        checks.expect(
            &format!("claim-race-round-{round}-no-pod-in-two-boxes"),
            one.box_id != two.box_id,
            format!("{} and {}", one.box_id, two.box_id),
        );
        for opened in [&one, &two] {
            let pod = pool::pod(env, opened.box_id.as_str()).await?;
            let claimed = pod.as_ref().and_then(|p| pool::label(p, LABEL_POOL));
            checks.expect(
                &format!("claim-race-round-{round}-pod-is-claimed"),
                claimed.as_deref() == Some(POOL_CLAIMED),
                format!("{claimed:?}"),
            );
        }
        adapter::release(&left, &one.box_id).await?;
        adapter::release(&right, &two.box_id).await?;
        scale_pool(env, 0).await?;
        wait_until("pool empty", 120, || async {
            Ok(pool::warm_list(env, false).await?.is_empty())
        })
        .await?;
    }
    drop(keeper);
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_claim_moves_the_pod_out_of_the_replica_set_and_a_new_warm_pod_appears() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    pool::wait_warm(env, 1, 120).await?;
    let before = pool::warm_list(env, true).await?.len();
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let pod = pool::pod(env, opened.box_id.as_str())
        .await?
        .ok_or_else(|| battery::error::data("claimed pod", "absent"))?;
    let owners = pod["metadata"]["ownerReferences"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let warm = pool::warm_list(env, false).await?;
    checks.expect(
        "claim-pod-label-is-claimed",
        pool::label(&pod, LABEL_POOL).as_deref() == Some(POOL_CLAIMED),
        format!("{:?}", pool::label(&pod, LABEL_POOL)),
    );
    checks.expect(
        "claim-pod-no-longer-matches-the-selector",
        !warm.contains(&opened.box_id.to_string()),
        format!("{warm:?}"),
    );
    wait_until("the replica set to make a new warm pod", 120, || async {
        Ok(pool::warm_list(env, true).await?.len() >= before)
    })
    .await?;
    checks.expect(
        "claim-replica-set-made-a-new-warm-pod",
        true,
        format!("owners left on the claimed pod: {owners}"),
    );
    adapter::release(&sandbox, &opened.box_id).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cold_pod_from_the_template_starts_like_a_warm_pod_and_stays_out_of_the_replica_set()
-> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let keeper = empty_pool(env).await?;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let pod = pool::pod(env, opened.box_id.as_str())
        .await?
        .ok_or_else(|| battery::error::data("cold pod", "absent"))?;
    let template = pool::replica_set(env).await?;
    let names = |value: &serde_json::Value, path: &str| -> Vec<String> {
        value[path]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter_map(|c| c["name"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let spec = &template["spec"]["template"]["spec"];
    checks.expect(
        "cold-pod-has-the-containers-of-the-template",
        names(&pod["spec"], "containers") == names(spec, "containers"),
        format!("{:?}", names(&pod["spec"], "containers")),
    );
    checks.expect(
        "cold-pod-has-the-init-containers-of-the-template",
        names(&pod["spec"], "initContainers") == names(spec, "initContainers"),
        "init containers",
    );
    let template_volumes = names(spec, "volumes");
    let pod_volumes = names(&pod["spec"], "volumes");
    checks.expect(
        "cold-pod-has-the-volumes-of-the-template",
        template_volumes.iter().all(|v| pod_volumes.contains(v)),
        format!("{pod_volumes:?}"),
    );
    checks.expect(
        "cold-pod-has-the-service-account-of-the-template",
        pod["spec"]["serviceAccountName"] == spec["serviceAccountName"],
        format!("{}", pod["spec"]["serviceAccountName"]),
    );
    checks.expect(
        "cold-pod-is-claimed",
        pool::label(&pod, LABEL_POOL).as_deref() == Some(POOL_CLAIMED),
        "pool label",
    );
    checks.expect(
        "cold-pod-is-marked-not-clean",
        pool::annotation(&pod, ANNOTATION_CLEAN).as_deref() == Some(CLEAN_FALSE),
        format!("{:?}", pool::annotation(&pod, ANNOTATION_CLEAN)),
    );
    checks.expect(
        "cold-pod-has-the-role-label-of-the-template",
        pool::label(&pod, LABEL_ROLE)
            == template["spec"]["template"]["metadata"]["labels"][LABEL_ROLE]
                .as_str()
                .map(str::to_owned),
        "role label",
    );
    checks.expect(
        "cold-pod-has-no-owner-reference",
        pod["metadata"]["ownerReferences"]
            .as_array()
            .is_none_or(Vec::is_empty),
        format!("{}", pod["metadata"]["ownerReferences"]),
    );
    scale_pool(env, 1).await?;
    tokio::time::sleep(Duration::from_secs(20)).await;
    checks.expect(
        "replica-set-does-not-delete-the-cold-pod",
        pool::pod(env, opened.box_id.as_str()).await?.is_some(),
        "pod present after 20 s",
    );
    adapter::release(&sandbox, &opened.box_id).await?;
    drop(keeper);
    checks.finish()
}
