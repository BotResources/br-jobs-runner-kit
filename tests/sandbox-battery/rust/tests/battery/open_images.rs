use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter::{self, Tunables, request};
use battery::common::wait_until;
use battery::error::Res;
use battery::evidence::Checks;
use battery::guards::{CordonGuard, ValueGuard};
use battery::ids::Lane;
use battery::pool;
use br_agentic_harness_contract::{OpenFault, SandboxPort, Seed};
use br_runner_sandbox::{Error, ImageError, LABEL_POOL, POOL_CLAIMED};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pool_whose_image_fails_gives_image_failing_at_once() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut values = ValueGuard::new(env);
    let mut checks = Checks::new(env);
    values
        .set(&[(env.image_key.clone(), json!("registry.invalid/none:1"))])
        .await?;
    for name in pool::warm_list(env, false).await? {
        pool::delete_now(env, &name).await?;
    }
    let started = Instant::now();
    wait_until("the condition WarmPodsFailing", 300, || async {
        Ok(pool::replica_set(env).await?["status"]["conditions"]
            .as_array()
            .is_some_and(|all| {
                all.iter()
                    .any(|c| c["type"] == "WarmPodsFailing" && c["status"] == "True")
            }))
    })
    .await?;
    env.record(
        "image_failing_upgrade_to_condition_seconds",
        format!("{:.1}", started.elapsed().as_secs_f64()),
        "s",
    );
    let mut tunables = Tunables::standard(env);
    tunables.open_timeout = 20;
    let sandbox = tunables.connect(env).await?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let (_, turn) = Lane::new()?.first()?;
    let request = request(turn, Seed::empty(), None)?;
    let began = Instant::now();
    let outcome = sandbox.open_box(&request).await;
    let seconds = began.elapsed().as_secs_f64();
    env.record(
        "image_failing_open_error_seconds",
        format!("{seconds:.2}"),
        "s",
    );
    let condition = pool::replica_set(env).await?["status"]["conditions"]
        .as_array()
        .and_then(|all| all.iter().find(|c| c["type"] == "WarmPodsFailing").cloned())
        .unwrap_or_default();
    let same = matches!(&outcome, Err(Error::Image(ImageError::Failing { reason, message }))
        if Some(reason.as_str()) == condition["reason"].as_str() && Some(message.as_str()) == condition["message"].as_str());
    checks.expect(
        "image-failing-open-ends-before-the-open-timeout",
        seconds < 20.0,
        format!("{seconds:.2} s"),
    );
    checks.expect(
        "image-failing-carries-reason-and-message-of-the-condition",
        same,
        format!("{outcome:?} against {condition}"),
    );
    let port = sandbox.open(&request).await;
    checks.expect(
        "image-failing-is-found-by-open-fault-of",
        matches!(&port, Err(e) if OpenFault::of(e) == Some(OpenFault::ImageFailing)),
        format!("{port:?}"),
    );
    let cold = pool::all_pods(env).await?["items"]
        .as_array()
        .map(|all| {
            all.iter()
                .filter(|p| pool::label(p, LABEL_POOL).as_deref() == Some(POOL_CLAIMED))
                .count()
        })
        .unwrap_or(0);
    checks.expect(
        "image-failing-makes-no-cold-pod",
        cold == 0,
        format!("{cold} claimed pods"),
    );
    drop(values);
    for name in pool::warm_list(env, false).await? {
        pool::delete_now(env, &name).await?;
    }
    pool::wait_warm(env, 1, 300).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pod_that_never_gets_ready_gives_image_unavailable_at_the_open_timeout() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut checks = Checks::new(env);
    let mut tunables = Tunables::standard(env);
    tunables.open_timeout = 10;
    let sandbox = tunables.connect(env).await?;
    let cordon = CordonGuard::cordon(env).await?;
    for name in pool::warm_list(env, false).await? {
        pool::delete_now(env, &name).await?;
    }
    wait_until("no ready warm pod", 60, || async {
        Ok(pool::warm_list(env, true).await?.is_empty())
    })
    .await?;
    let (_, turn) = Lane::new()?.first()?;
    let request = request(turn, Seed::empty(), None)?;
    let began = Instant::now();
    let outcome = sandbox.open_box(&request).await;
    let seconds = began.elapsed().as_secs_f64();
    env.record(
        "image_unavailable_open_seconds",
        format!("{seconds:.2}"),
        "s",
    );
    checks.expect(
        "image-unavailable-ends-near-the-open-timeout",
        matches!(&outcome, Err(Error::Image(ImageError::Unavailable { .. })))
            && (9.0..=11.0).contains(&seconds),
        format!("{seconds:.2} s {outcome:?}"),
    );
    let condition = pool::replica_set(env).await?["status"]["conditions"]
        .as_array()
        .is_some_and(|all| {
            all.iter()
                .any(|c| c["type"] == "WarmPodsFailing" && c["status"] == "True")
        });
    checks.expect(
        "image-unavailable-writes-no-failing-pool-condition",
        !condition,
        format!("{condition}"),
    );
    let leftover = wait_until("the cold pod to be deleted", 60, || async {
        Ok(pool::all_pods(env).await?["items"]
            .as_array()
            .is_none_or(|all| {
                all.iter()
                    .all(|p| pool::label(p, LABEL_POOL).as_deref() != Some(POOL_CLAIMED))
            }))
    })
    .await;
    checks.expect(
        "image-unavailable-deletes-the-cold-pod",
        leftover.is_ok(),
        format!("{leftover:?}"),
    );
    drop(cordon);
    pool::wait_warm(env, 1, 180).await?;
    let retry = adapter::open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await;
    checks.expect(
        "image-unavailable-retry-succeeds-after-the-cause-is-gone",
        retry.is_ok(),
        format!("{retry:?}"),
    );
    if let Ok(opened) = retry {
        adapter::release(&sandbox, &opened.box_id).await?;
    }
    checks.finish()
}
