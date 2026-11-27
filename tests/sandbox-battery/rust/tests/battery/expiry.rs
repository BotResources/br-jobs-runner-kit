use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter::{self, Tunables, sh_ok};
use battery::common::work_hash;
use battery::error::Res;
use battery::evidence::Checks;
use battery::guards::{ScaleGuard, ValueGuard};
use battery::ids::Lane;
use battery::proc::{self, Helm, strings};
use battery::saves::Saves;
use br_agentic_harness_contract::{FreshReason, Seed, StartKind};
use br_runner_sandbox::{ConfigError, SandboxConfig, Store, tombstone_key};
use serde_json::json;

const TTL_KEY: &str = "save.ttl_seconds";

async fn turn(
    sandbox: &br_runner_sandbox::Sandbox,
    lane: &Lane,
    parent: Option<br_agentic_harness_contract::TurnId>,
    marker: &str,
) -> Res<(br_agentic_harness_contract::TurnId, String)> {
    let (id, request) = match parent {
        Some(p) => lane.child(p)?,
        None => lane.first()?,
    };
    let opened = adapter::open(sandbox, request, Seed::empty()).await?;
    sh_ok(
        sandbox,
        &opened.box_id,
        &format!("printf {marker} > /work/only-in-{marker}"),
    )
    .await?;
    let hash = work_hash(sandbox, &opened.box_id).await?;
    adapter::save(sandbox, &opened.box_id).await?;
    adapter::release(sandbox, &opened.box_id).await?;
    Ok((id, hash))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_save_expires_after_60_seconds_and_a_later_save_is_a_whole_image() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut values = ValueGuard::new(env);
    values.set(&[(TTL_KEY.to_owned(), json!(60))]).await?;
    let mut tunables = Tunables::standard(env);
    tunables.save_ttl = 60;
    let sandbox = tunables.connect(env).await?;
    let saves = Saves::through(env, &tunables)?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (k1, _) = turn(&sandbox, &lane, None, "1").await?;
    let made = Instant::now();
    tokio::time::sleep(Duration::from_secs(20)).await;
    let (k2, _) = turn(&sandbox, &lane, Some(k1), "2").await?;
    tokio::time::sleep(Duration::from_secs(20)).await;
    let (k3, h3) = turn(&sandbox, &lane, Some(k2), "3").await?;
    let mut sizes = Vec::new();
    for id in [k1, k2, k3] {
        sizes.push(
            saves
                .stored_bytes(&saves.key(lane.thread, lane.agent, id))
                .await?,
        );
    }
    env.record(
        "expiry_object_sizes_k1_k2_k3",
        format!("{sizes:?}"),
        "bytes",
    );
    let key1 = saves.key(lane.thread, lane.agent, k1);
    let limit = 60 + 60 + 60 + 60;
    let mut latest = k3;
    let mut latest_hash = h3;
    let mut count = 3;
    let mut last_save = Instant::now();
    let gone = loop {
        if saves.manifest(&key1).await?.is_none() {
            break true;
        }
        if made.elapsed().as_secs() > limit {
            break false;
        }
        if last_save.elapsed().as_secs() >= 20 {
            count += 1;
            let (id, hash) = turn(&sandbox, &lane, Some(latest), &count.to_string()).await?;
            latest = id;
            latest_hash = hash;
            last_save = Instant::now();
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    };
    let delay = made.elapsed().as_secs_f64();
    env.record(
        "expiry_k1_deleted_after_seconds",
        format!("{delay:.0}"),
        "s",
    );
    env.record("expiry_saves_made_until_k1_was_gone", count, "count");
    checks.expect(
        "expiry-manifest-of-k1-is-gone-within-ttl-skew-and-sweep",
        gone,
        format!("{delay:.0} s"),
    );
    checks.expect(
        "expiry-tombstone-exists-for-k1",
        saves.store.get(&tombstone_key(&key1)).await?.is_some(),
        tombstone_key(&key1),
    );
    let (_, next) = lane.child(latest)?;
    let again = adapter::open(&sandbox, next, Seed::empty()).await?;
    let hash = work_hash(&sandbox, &again.box_id).await?;
    let listing = sh_ok(&sandbox, &again.box_id, "ls /work").await?;
    let whole = (1..=count).all(|n| listing.contains(&format!("only-in-{n}")));
    checks.expect(
        "expiry-restore-of-a-later-save-after-k1-is-gone-is-the-whole-image",
        matches!(again.start, StartKind::Restored { turn } if turn == latest)
            && hash == latest_hash
            && whole,
        format!("{:?} {listing}", again.start),
    );
    adapter::release(&sandbox, &again.box_id).await?;
    let (_, after) = lane.child(k1)?;
    let expired = adapter::open(&sandbox, after, Seed::empty()).await?;
    checks.expect(
        "expiry-open-after-the-keeper-expired-k1-is-save-expired-with-a-time",
        matches!(
            expired.start,
            StartKind::Fresh(FreshReason::SaveExpired {
                expired_at: Some(_)
            })
        ),
        format!("{:?}", expired.start),
    );
    adapter::release(&sandbox, &expired.box_id).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_the_keeper_stopped_an_old_save_is_expired_on_the_read_side() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let keeper = ScaleGuard::scale_keeper(env, 0).await?;
    let mut tunables = Tunables::standard(env);
    tunables.save_ttl = 60;
    let sandbox = tunables.connect(env).await?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (k2, _) = turn(&sandbox, &lane, None, "2").await?;
    tokio::time::sleep(Duration::from_secs(65)).await;
    let (_, next) = lane.child(k2)?;
    let opened = adapter::open(&sandbox, next, Seed::empty()).await?;
    checks.expect(
        "expiry-read-side-open-is-save-expired",
        matches!(
            opened.start,
            StartKind::Fresh(FreshReason::SaveExpired {
                expired_at: Some(_)
            })
        ),
        format!("{:?}", opened.start),
    );
    adapter::release(&sandbox, &opened.box_id).await?;
    drop(keeper);
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_save_age_of_zero_or_less_is_refused_by_the_chart_and_the_adapter() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut checks = Checks::new(env);
    let helm = Helm::of(env);
    for value in [0, -1] {
        let mut args = strings(&[
            "--kube-context",
            &env.context,
            "template",
            &env.helm_release,
            &helm.chart,
            "-f",
            &env.values_file.display().to_string(),
        ]);
        args.push("--set-json".to_owned());
        args.push(format!("{TTL_KEY}={value}"));
        let output = proc::run("helm", &args, None, proc::LONG).await?;
        checks.expect(
            &format!("save-age-{value}-helm-template-fails"),
            output.code != 0,
            output.stderr.chars().take(200).collect::<String>(),
        );
    }
    let mut tunables = Tunables::standard(env);
    tunables.save_ttl = 0;
    let config = SandboxConfig::try_from(tunables.input(env)?);
    checks.expect(
        "save-age-0-adapter-config-error",
        matches!(config, Err(ConfigError::NotAboveZero { .. })),
        format!("{config:?}"),
    );
    checks.finish()
}
