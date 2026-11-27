use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter::{self, Tunables, request, sh_ok};
use battery::common::{tunables_from_chart, wait_until};
use battery::error::{Res, data};
use battery::evidence::Checks;
use battery::guards::PodReaper;
use battery::ids::Lane;
use battery::pool;
use battery::proc::Helm;
use battery::tree;
use battery::tunnel::{Gate, GateMode, api_gate};
use br_agentic_harness_contract::{Seed, StartKind};
use br_runner_sandbox::Error;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_warm_pod_watch_survives_a_break_of_the_api_connection() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let (gate, context) = api_gate(env, "battery-api-cut").await?;
    let mut tunables = Tunables::standard(env);
    tunables.context = context.name.clone();
    tunables.open_timeout = 20;
    tunables.cold_create = false;
    let sandbox = tunables.connect(env).await?;
    let mut checks = Checks::new(env);
    pool::wait_warm(env, 1, 120).await?;
    let prime = adapter::open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await?;
    adapter::release(&sandbox, &prime.box_id).await?;
    gate.set(GateMode::Refuse).await?;
    let runner = sandbox.clone();
    let during = request(Lane::new()?.first()?.1, Seed::empty(), None)?;
    let started = Instant::now();
    let cut_open =
        tokio::spawn(async move { runner.open_box(&during).await.map(|o| o.box_id.to_string()) });
    tokio::time::sleep(Duration::from_secs(60)).await;
    let cut_result = cut_open
        .await
        .map_err(|e| data("open during the cut", e.to_string()))?;
    env.record(
        "api_cut_open_during_cut_seconds",
        format!("{:.1}", started.elapsed().as_secs_f64()),
        "s",
    );
    checks.expect(
        "api-cut-open-during-the-cut-is-a-fault-not-an-empty-pool",
        cut_result.is_err() && !matches!(&cut_result, Err(Error::Image(_))),
        format!("{cut_result:?}"),
    );
    gate.set(GateMode::Open).await?;
    pool::wait_warm(env, 1, 120).await?;
    tokio::time::sleep(Duration::from_secs(10)).await;
    let after = adapter::open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await;
    let exists = match &after {
        Ok(opened) => {
            !pool::is_cold(opened.box_id.as_str())
                && pool::pod(env, opened.box_id.as_str()).await?.is_some()
        }
        Err(_) => false,
    };
    checks.expect(
        "api-cut-open-after-the-restore-claims-a-warm-pod",
        exists,
        format!("{after:?}"),
    );
    if let Ok(opened) = after {
        adapter::release(&sandbox, &opened.box_id).await?;
    }
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_or_lost_kube_api_gives_a_fault_not_a_stuck_call() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut checks = Checks::new(env);
    let mut reaper = PodReaper::new(env);
    for (name, mode) in [("lost", GateMode::Refuse), ("stalled", GateMode::Stall)] {
        let (gate, context) = api_gate(env, &format!("battery-api-{name}")).await?;
        let mut tunables = Tunables::standard(env);
        tunables.context = context.name.clone();
        tunables.open_timeout = 10;
        let sandbox = tunables.connect(env).await?;
        pool::wait_warm(env, 1, 120).await?;
        let prime = adapter::open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await?;
        reaper.track(prime.box_id.to_string());
        adapter::release(&sandbox, &prime.box_id).await?;
        let claimed_before = pool::claimed_list(env).await?;
        gate.set(mode).await?;
        let req = request(Lane::new()?.first()?.1, Seed::empty(), None)?;
        let limit = Duration::from_secs(10 + 60);
        let started = Instant::now();
        let outcome = tokio::time::timeout(limit, sandbox.open_box(&req)).await;
        let seconds = started.elapsed().as_secs_f64();
        env.record(
            &format!("kube_api_{name}_open_seconds"),
            format!("{seconds:.2}"),
            "s",
        );
        if let Ok(Ok(opened)) = &outcome {
            reaper.track(opened.box_id.to_string());
        }
        match outcome {
            Ok(result) => checks.expect(
                &format!("kube-api-{name}-gives-a-fault-within-the-open-timeout"),
                result.is_err() && seconds <= 15.0,
                format!("{seconds:.2} s, {:?}", result.map(|o| o.box_id)),
            ),
            Err(_) => checks.expect(
                &format!("kube-api-{name}-gives-a-fault-within-the-open-timeout"),
                false,
                format!("hang after {} s", limit.as_secs()),
            ),
        }
        gate.set(GateMode::Open).await?;
        for pod in pool::claimed_list(env).await? {
            if !claimed_before.contains(&pod) {
                reaper.track(pod);
            }
        }
        pool::wait_warm(env, 1, 120).await?;
    }
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_live_pod_is_reused_with_its_files_and_kept_only_for_the_idle_time() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let store_upstream = env.s3.endpoint.trim_start_matches("http://").to_owned();
    let gate = Gate::start(store_upstream, false).await?;
    let mut tunables = tunables_from_chart(env).await?;
    let keep_min = Helm::of(env)
        .values()
        .await?
        .get("idle_keep_min_state_bytes")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| data("the chart values", "idle_keep_min_state_bytes is not set"))?;
    tunables.idle_keep_min = keep_min;
    tunables.store_endpoint = format!("http://127.0.0.1:{}", gate.port());
    let sandbox = tunables.connect(env).await?;
    let mut reaper = PodReaper::new(env);
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (first, turn) = lane.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    reaper.track(opened.box_id.to_string());
    sh_ok(&sandbox, &opened.box_id, "printf kept > /work/live").await?;
    tree::state(&sandbox, &opened.box_id, 5, keep_min).await?;
    adapter::save(&sandbox, &opened.box_id).await?;
    adapter::release(&sandbox, &opened.box_id).await?;
    gate.set(GateMode::Refuse).await?;
    let (_, next) = lane.child(first)?;
    let again = adapter::open(&sandbox, next, Seed::empty()).await?;
    let content = sh_ok(&sandbox, &again.box_id, "cat /work/live").await?;
    checks.expect(
        "live-reuse-claims-the-same-pod",
        again.box_id == opened.box_id,
        format!("{} and {}", opened.box_id, again.box_id),
    );
    checks.expect(
        "live-reuse-start-is-continued-with-no-store-read",
        again.start == StartKind::Continued,
        format!("{:?}", again.start),
    );
    checks.expect("live-reuse-files-are-there", content == "kept", content);
    gate.set(GateMode::Open).await?;
    adapter::save(&sandbox, &again.box_id).await?;
    adapter::release(&sandbox, &again.box_id).await?;
    let released = Instant::now();
    let name = opened.box_id.to_string();
    let limit = tunables.idle_ttl + 120;
    wait_until("the keeper to delete the idle pod", limit, || async {
        Ok(pool::pod(env, &name)
            .await?
            .is_none_or(|p| !p["metadata"]["deletionTimestamp"].is_null()))
    })
    .await?;
    let kept_for = released.elapsed().as_secs_f64();
    env.record(
        "idle_pod_deleted_after_release_seconds",
        format!("{kept_for:.1}"),
        "s",
    );
    checks.expect(
        "idle-pod-is-deleted-after-the-idle-time",
        kept_for >= tunables.idle_ttl as f64 - 2.0,
        format!("kept {kept_for:.1} s, idle_ttl {} s", tunables.idle_ttl),
    );
    checks.finish()
}
