use std::process::Stdio;
use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter;
use battery::common::{restored, wait_until, work_hash};
use battery::error::{Res, data};
use battery::evidence::Checks;
use battery::ids::Lane;
use battery::kubeconfig::TempContext;
use battery::saves::Saves;
use battery::tunnel::{Gate, GateMode};
use br_agentic_harness_contract::{Seed, TurnId};
use br_runner_sandbox::{Sandbox, Store};

const STATE_BYTES: u64 = 60_000_000;
const SWEEP_WAIT_SECONDS: u64 = 900;

async fn prepare(sandbox: &Sandbox, lane: &Lane) -> Res<(TurnId, String)> {
    let (k1, turn) = lane.first()?;
    let opened = adapter::open(sandbox, turn, Seed::empty()).await?;
    adapter::sh_ok(sandbox, &opened.box_id, "printf one > /work/one").await?;
    let hash = work_hash(sandbox, &opened.box_id).await?;
    adapter::save(sandbox, &opened.box_id).await?;
    adapter::release(sandbox, &opened.box_id).await?;
    Ok((k1, hash))
}

fn child(
    lane: &Lane,
    turn: TurnId,
    parent: TurnId,
    store: &str,
    context: &str,
) -> Res<tokio::process::Child> {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_save_child"))
        .args([
            lane.thread.to_string(),
            lane.agent.to_string(),
            turn.to_string(),
            parent.to_string(),
            store.to_owned(),
            context.to_owned(),
            STATE_BYTES.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| data("save child", e.to_string()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_crash_after_the_parts_and_before_the_manifest_leaves_no_save() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (k1, h1) = prepare(&sandbox, &lane).await?;
    let gate = Gate::start(
        env.s3.endpoint.trim_start_matches("http://").to_owned(),
        true,
    )
    .await?;
    gate.set(GateMode::HoldManifestWrite);
    let (k2, _) = lane.child(k1)?;
    let mut process = child(
        &lane,
        k2,
        k1,
        &format!("http://127.0.0.1:{}", gate.port()),
        &env.context,
    )?;
    let prefix = format!(
        "saves/{}/{}/{}/{}/",
        saves.client, lane.thread, lane.agent, k2
    );
    wait_until(
        "the part of the crashing save to be uploaded",
        300,
        || async { Ok(!saves.store.list(&prefix).await?.is_empty()) },
    )
    .await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    process
        .kill()
        .await
        .map_err(|e| data("kill child", e.to_string()))?;
    gate.set(GateMode::Open);
    let key = saves.key(lane.thread, lane.agent, k2);
    checks.expect(
        "crash-before-manifest-has-no-manifest",
        saves.manifest(&key).await?.is_none(),
        "manifest of k2",
    );
    let listed = saves.objects(lane.thread).await?;
    let manifests = listed
        .saves
        .iter()
        .filter(|k| k.ends_with("manifest.json"))
        .count();
    checks.expect(
        "crash-before-manifest-lists-only-complete-saves",
        manifests == 1,
        format!("{manifests} manifests"),
    );
    let (_, turn) = lane.child(k1)?;
    let again = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let hash = work_hash(&sandbox, &again.box_id).await?;
    checks.expect(
        "crash-before-manifest-next-open-restores-k1",
        restored(&again.start, k1) && hash == h1,
        format!("{:?}", again.start),
    );
    adapter::release(&sandbox, &again.box_id).await?;
    let started = Instant::now();
    let swept = wait_until(
        "the keeper to sweep the orphan part",
        SWEEP_WAIT_SECONDS,
        || async { Ok(saves.store.list(&prefix).await?.is_empty()) },
    )
    .await;
    env.record(
        "orphan_part_sweep_seconds",
        format!("{:.0}", started.elapsed().as_secs_f64()),
        "s",
    );
    checks.expect(
        "crash-before-manifest-orphan-part-is-swept",
        swept.is_ok(),
        format!("{swept:?}"),
    );
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_crash_after_the_manifest_and_before_the_pod_annotation_restores_from_the_manifest() -> Res
{
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (k1, _) = prepare(&sandbox, &lane).await?;
    let raw = battery::proc::Kube::cluster_wide(env)
        .run(&[
            "config",
            "view",
            "--raw",
            "--minify",
            "--context",
            &env.context,
            "-o",
            "jsonpath={.clusters[0].cluster.server}",
        ])
        .await?;
    let gate = Gate::start(
        raw.trim_start_matches("https://")
            .replace("0.0.0.0", "127.0.0.1"),
        false,
    )
    .await?;
    let context = TempContext::through_gate(env, "battery-crash-api", gate.port()).await?;
    let (k2, _) = lane.child(k1)?;
    let mut process = child(&lane, k2, k1, &env.s3.endpoint, &context.name)?;
    let key = saves.key(lane.thread, lane.agent, k2);
    wait_until("the manifest of the save", 300, || async {
        Ok(saves.manifest(&key).await?.is_some())
    })
    .await?;
    gate.set(GateMode::Stall);
    process
        .kill()
        .await
        .map_err(|e| data("kill child", e.to_string()))?;
    gate.set(GateMode::Refuse);
    let (_, turn) = lane.child(k2)?;
    let again = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let content = adapter::sh_ok(&sandbox, &again.box_id, "cat /work/one; ls /work").await?;
    env.record(
        "crash_after_manifest_restored_turn_is_k2",
        restored(&again.start, k2),
        "bool",
    );
    checks.expect(
        "crash-after-manifest-next-open-restores-from-the-manifest",
        restored(&again.start, k2),
        format!("{:?} {content}", again.start),
    );
    adapter::release(&sandbox, &again.box_id).await?;
    checks.finish()
}
