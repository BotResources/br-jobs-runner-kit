use std::time::Duration;

use br_agentic_harness_contract::{BoxId, SaveReceipt, StartKind, TurnId};
use br_runner_sandbox::Sandbox;
use serde_json::{Value, json};

use crate::adapter::{self, Tunables};
use crate::env::Env;
use crate::error::{Res, data};
use crate::guards::{ScaleGuard, ValueGuard};
use crate::ids::Lane;
use crate::pool;
use crate::proc::{self, Kube, SHORT, strings};
use crate::seeds;
use crate::tree;

pub fn floor_key(env: &Env) -> String {
    format!("clients.{}.modes.{}.pool.floor", env.client, env.mode)
}

pub async fn ensure_floor(env: &Env, guard: &mut ValueGuard, want: u64) -> Res {
    let current = guard
        .current(&floor_key(env))
        .await?
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    if current < want {
        let max_key = floor_key(env).replace(".floor", ".max_sandboxes");
        let max = guard
            .current(&max_key)
            .await?
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let mut sets = vec![
            (floor_key(env), json!(want)),
            ("quota.extra_pods".to_owned(), json!(want + 4)),
        ];
        if max < want {
            sets.push((max_key, json!(want)));
        }
        guard.set(&sets).await?;
    }
    pool::wait_warm(env, want as usize, 180).await
}

pub async fn restore_floor_wait(env: &Env, want: usize) -> Res {
    pool::wait_warm(env, want, 120).await
}

pub async fn state_turn(
    sandbox: &Sandbox,
    lane: &Lane,
    parent: Option<TurnId>,
    seed: u64,
    bytes: u64,
) -> Res<(TurnId, SaveReceipt)> {
    let (id, turn) = match parent {
        Some(parent) => lane.child(parent)?,
        None => lane.first()?,
    };
    let opened = adapter::open(sandbox, turn, br_agentic_harness_contract::Seed::empty()).await?;
    tree::state(sandbox, &opened.box_id, seed, bytes).await?;
    let receipt = adapter::save(sandbox, &opened.box_id).await?;
    adapter::release(sandbox, &opened.box_id).await?;
    Ok((id, receipt))
}

pub async fn true_exit(sandbox: &Sandbox, boxed: &BoxId) -> Res<i32> {
    Ok(adapter::sh(sandbox, boxed, "true").await?.exit_code)
}

pub fn restored(start: &StartKind, turn: TurnId) -> bool {
    matches!(start, StartKind::Restored { turn: got } if *got == turn)
}

pub async fn tunables_from_chart(env: &Env) -> Res<Tunables> {
    let values = proc::Helm::of(env).values().await?;
    let mut tunables = Tunables::standard(env);
    if let Some(ttl) = proc::value_at(&values, "save.ttl_seconds").and_then(Value::as_u64) {
        tunables.save_ttl = ttl;
    }
    if let Some(idle) = values.get("idle_ttl").and_then(Value::as_u64) {
        tunables.idle_ttl = idle;
    }
    Ok(tunables)
}

pub async fn pending_uploads(env: &Env, prefix: &str) -> Res<usize> {
    let url = format!(
        "{}/{}/?uploads&prefix={}",
        env.s3.endpoint.trim_end_matches('/'),
        env.s3.bucket,
        prefix
    );
    let args = strings(&[
        "-sS",
        "--fail",
        "--aws-sigv4",
        &format!("aws:amz:{}:s3", env.s3.region),
        "--user",
        &format!("{}:{}", env.s3.access_key, env.s3.secret_key),
        &url,
    ]);
    let out = proc::run("curl", &args, None, SHORT).await?;
    let out = proc::succeeded("curl", &args, out)?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .matches("<Upload>")
        .count())
}

pub async fn scale_pool(env: &Env, replicas: u64) -> Res {
    Kube::of(env)
        .run(&[
            "scale",
            "replicaset",
            &env.replica_set(),
            &format!("--replicas={replicas}"),
        ])
        .await
        .map(|_| ())
}

pub async fn wait_until<F, Fut>(what: &str, seconds: u64, mut probe: F) -> Res
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Res<bool>>,
{
    let start = std::time::Instant::now();
    loop {
        if probe().await? {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            return Err(crate::error::BatteryError::NotReached {
                what: what.to_owned(),
                seconds,
            });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub fn seed_standard() -> Res<seeds::SeedSet> {
    seeds::standard()
}

pub async fn work_hash(sandbox: &Sandbox, boxed: &BoxId) -> Res<String> {
    let listing = adapter::sh_ok(
        sandbox,
        boxed,
        "cd /work && find . -type f -exec sha256sum {} + | sort -k2",
    )
    .await?;
    Ok(seeds::sha256_hex(listing.as_bytes()))
}

pub async fn timed_open(
    sandbox: &Sandbox,
    turn: br_agentic_harness_contract::TurnRef,
    seed: br_agentic_harness_contract::Seed,
) -> Res<(f64, br_agentic_harness_contract::Opened)> {
    let started = std::time::Instant::now();
    let opened = adapter::open(sandbox, turn, seed).await?;
    let exit = true_exit(sandbox, &opened.box_id).await?;
    let millis = started.elapsed().as_secs_f64() * 1000.0;
    if exit != 0 {
        return Err(data("first true", format!("exit code {exit}")));
    }
    Ok((millis, opened))
}

pub fn record_stats(env: &Env, name: &str, stats: &crate::prng::Stats) {
    env.record(
        &format!("{name}_median"),
        format!("{:.0}", stats.median),
        "ms",
    );
    env.record(&format!("{name}_p95"), format!("{:.0}", stats.p95), "ms");
    env.record(
        &format!("{name}_slowest"),
        format!("{:.0}", stats.slowest),
        "ms",
    );
    env.record(
        &format!("{name}_within_500ms"),
        stats.within,
        &format!("of {}", stats.count),
    );
}

pub async fn empty_pool(env: &Env) -> Res<ScaleGuard> {
    let keeper = ScaleGuard::scale_keeper(env, 0).await?;
    scale_pool(env, 0).await?;
    wait_until("no warm pod", 120, || async {
        Ok(pool::warm_list(env, false).await?.is_empty())
    })
    .await?;
    Ok(keeper)
}

pub async fn settle() -> Res {
    let env = Env::load()?;
    pool::wait_gone_all(&env).await?;
    let floor = ValueGuard::new(&env)
        .current(&floor_key(&env))
        .await?
        .and_then(|value| value.as_u64())
        .unwrap_or(1) as usize;
    pool::wait_warm(&env, floor, 120).await
}
