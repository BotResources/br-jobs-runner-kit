use std::time::Duration;

use battery::Session;
use battery::adapter::{self, Tunables, sh_ok};
use battery::common::{state_turn, wait_until};
use battery::error::Res;
use battery::evidence::Checks;
use battery::guards::PodReaper;
use battery::ids::Lane;
use battery::kubeconfig::TempContext;
use battery::pool;
use br_agentic_harness_contract::Seed;
use br_runner_sandbox::{ANNOTATION_IDLE_SINCE, LABEL_POOL, POOL_CLAIMED, Timestamp};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_release_deletes_a_small_box_and_keeps_a_large_saved_one_idle() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut tunables = Tunables::standard(env);
    tunables.idle_keep_min = 10_000_000;
    let sandbox = tunables.connect(env).await?;
    let mut reaper = PodReaper::new(env);
    let mut checks = Checks::new(env);
    let small = adapter::open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await?;
    reaper.track(small.box_id.to_string());
    sh_ok(&sandbox, &small.box_id, "printf small > /work/s").await?;
    adapter::save(&sandbox, &small.box_id).await?;
    adapter::release(&sandbox, &small.box_id).await?;
    let large = adapter::open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await?;
    reaper.track(large.box_id.to_string());
    battery::tree::state(&sandbox, &large.box_id, 9, 30_000_000).await?;
    adapter::save(&sandbox, &large.box_id).await?;
    let released = std::time::SystemTime::now();
    adapter::release(&sandbox, &large.box_id).await?;
    wait_until("the small pod to be deleted", 60, || async {
        Ok(pool::pod(env, small.box_id.as_str())
            .await?
            .is_none_or(|p| !p["metadata"]["deletionTimestamp"].is_null()))
    })
    .await?;
    checks.expect(
        "release-deletes-the-pod-of-a-small-save",
        true,
        small.box_id.to_string(),
    );
    let pod = pool::pod(env, large.box_id.as_str()).await?;
    let idle = pod
        .as_ref()
        .and_then(|p| pool::annotation(p, ANNOTATION_IDLE_SINCE));
    let near = idle
        .as_deref()
        .and_then(|text| Timestamp::parse(text).ok())
        .map(|stamp| {
            let at = stamp.to_system_time();
            let gap = at.duration_since(released).unwrap_or_else(|e| e.duration());
            gap < Duration::from_secs(10)
        })
        .unwrap_or(false);
    let kept = pod
        .as_ref()
        .is_some_and(|p| p["metadata"]["deletionTimestamp"].is_null());
    checks.expect(
        "release-keeps-the-pod-of-a-large-save-with-idle-since",
        kept && near,
        format!("{idle:?}"),
    );
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_adapter_works_with_only_the_rights_of_the_intent() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let subject = format!("system:serviceaccount:{}:{}", env.runner_ns, env.runner_sa);
    let context = TempContext::impersonating(env, "battery-runner", &subject).await?;
    let namespace = env.namespace();
    let mut checks = Checks::new(env);
    for (verb, resource) in [
        ("get", "pods"),
        ("list", "pods"),
        ("watch", "pods"),
        ("patch", "pods"),
        ("create", "pods"),
        ("delete", "pods"),
        ("create", "pods/exec"),
        ("get", "replicasets"),
    ] {
        let allowed = context.can_i(&namespace, verb, resource).await?;
        checks.expect(
            &format!("runner-can-{verb}-{resource}"),
            allowed,
            subject.clone(),
        );
    }
    for (verb, resource) in [
        ("get", "secrets"),
        ("delete", "replicasets"),
        ("create", "deployments"),
    ] {
        let allowed = context.can_i(&namespace, verb, resource).await?;
        checks.expect(
            &format!("runner-cannot-{verb}-{resource}"),
            !allowed,
            subject.clone(),
        );
    }
    let mut tunables = Tunables::standard(env);
    tunables.context = context.name.clone();
    tunables.idle_keep_min = 1;
    let sandbox = tunables.connect(env).await?;
    pool::wait_warm(env, 1, 120).await?;
    let lane = Lane::new()?;
    let (first, turn) = lane.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    sh_ok(&sandbox, &opened.box_id, "printf rights > /work/r").await?;
    adapter::save(&sandbox, &opened.box_id).await?;
    adapter::release(&sandbox, &opened.box_id).await?;
    let (_, next) = lane.child(first)?;
    let again = adapter::open(&sandbox, next, Seed::empty()).await?;
    adapter::release(&sandbox, &again.box_id).await?;
    let erased = sandbox.erase(lane.thread).await;
    checks.expect(
        "runner-identity-open-save-release-erase-work",
        erased.is_ok(),
        format!("{erased:?}"),
    );
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_watch_follows_deleted_and_new_warm_pods() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    pool::wait_warm(env, 1, 120).await?;
    let prime = adapter::open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await?;
    adapter::release(&sandbox, &prime.box_id).await?;
    let old = pool::warm_list(env, false).await?;
    for name in &old {
        pool::delete_now(env, name).await?;
    }
    pool::wait_warm(env, 1, 120).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let opened = adapter::open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await?;
    let fresh = !old.contains(&opened.box_id.to_string());
    let claimed = pool::pod(env, opened.box_id.as_str())
        .await?
        .and_then(|p| pool::label(&p, LABEL_POOL));
    checks.expect(
        "watch-claims-a-pod-made-after-the-deletes",
        fresh && claimed.as_deref() == Some(POOL_CLAIMED),
        format!("{} {claimed:?}", opened.box_id),
    );
    adapter::release(&sandbox, &opened.box_id).await?;
    let _ = state_turn;
    checks.finish()
}
