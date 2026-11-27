use std::mem::discriminant;

use battery::Session;
use battery::adapter::{self, Tunables, request, sh_ok};
use battery::common::{restored, state_turn};
use battery::error::Res;
use battery::evidence::Checks;
use battery::guards::ScaleGuard;
use battery::ids::{self, Lane};
use battery::pool;
use battery::prng::Prng;
use battery::saves::Saves;
use battery::tunnel::{Gate, GateMode};
use br_agentic_harness_contract::{FreshReason, Seed, StartKind};
use br_runner_sandbox::{
    AttemptId, Error, Record, Sandbox, Store, Timestamp, manifest_key, record_key, tombstone_key,
};
use bytes::Bytes;

async fn start_of(
    sandbox: &Sandbox,
    lane: &Lane,
    parent: Option<br_agentic_harness_contract::TurnId>,
) -> Res<(StartKind, String)> {
    let turn = match parent {
        Some(p) => lane.child(p)?.1,
        None => lane.first()?.1,
    };
    let opened = adapter::open(sandbox, turn, Seed::empty()).await?;
    let name = opened.box_id.to_string();
    adapter::release(sandbox, &opened.box_id).await?;
    battery::common::settle().await?;
    Ok((opened.start, name))
}

fn fresh(start: StartKind) -> Option<FreshReason> {
    match start {
        StartKind::Fresh(reason) => Some(reason),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_named_start_result_comes_from_its_scripted_scenario() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let store_gate = Gate::start(
        env.s3.endpoint.trim_start_matches("http://").to_owned(),
        true,
    )
    .await?;
    let mut tunables = Tunables::standard(env);
    tunables.store_endpoint = format!("http://127.0.0.1:{}", store_gate.port());
    tunables.save_ttl = 60;
    let sandbox = tunables.connect(env).await?;
    let saves = Saves::through(env, &tunables)?;
    let mut checks = Checks::new(env);
    let mut results: Vec<(&str, FreshReason)> = Vec::new();

    let lane = Lane::new()?;
    let (start, _) = start_of(&sandbox, &lane, None).await?;
    let no_parent = fresh(start);
    let (start, _) = start_of(&sandbox, &lane, Some(ids::turn()?)).await?;
    let unknown_parent = fresh(start);
    store_gate.set(GateMode::Refuse);
    let (start, _) = start_of(&sandbox, &lane, None).await?;
    store_gate.set(GateMode::Open);
    checks.expect(
        "start-no-save-ever-without-parent-and-with-unknown-parent-and-store-down",
        no_parent == Some(FreshReason::NoSaveEver)
            && unknown_parent == Some(FreshReason::NoSaveEver)
            && fresh(start) == Some(FreshReason::NoSaveEver),
        format!("{no_parent:?} {unknown_parent:?}"),
    );
    results.push(("NoSaveEver", FreshReason::NoSaveEver));

    let keeper = ScaleGuard::scale_keeper(env, 0).await?;
    let lane = Lane::new()?;
    let (k1, _) = state_turn(&sandbox, &lane, None, 51, 1000).await?;
    tokio::time::sleep(std::time::Duration::from_secs(65)).await;
    let (start, _) = start_of(&sandbox, &lane, Some(k1)).await?;
    let expired = fresh(start);
    checks.expect(
        "start-save-expired-with-a-time-on-the-read-side",
        matches!(
            expired,
            Some(FreshReason::SaveExpired {
                expired_at: Some(_)
            })
        ),
        format!("{expired:?}"),
    );
    drop(keeper);
    if let Some(reason) = expired {
        results.push(("SaveExpired", reason));
    }

    let mut long = Tunables::standard(env);
    long.store_endpoint = tunables.store_endpoint.clone();
    let long_sandbox = long.connect(env).await?;
    let lane = Lane::new()?;
    let (k1, _) = state_turn(&long_sandbox, &lane, None, 52, 1000).await?;
    let key = saves.key(lane.thread, lane.agent, k1);
    saves.delete_manifest_and_part(&key).await?;
    let _ = saves.store.delete(&tombstone_key(&key)).await;
    let (start, _) = start_of(&long_sandbox, &lane, Some(k1)).await?;
    checks.expect(
        "start-save-expired-without-a-time-when-only-the-record-is-left",
        fresh(start) == Some(FreshReason::SaveExpired { expired_at: None }),
        format!("{start:?}"),
    );

    let lane = Lane::new()?;
    let (k1, turn) = lane.first()?;
    let opened = adapter::open(&long_sandbox, turn, Seed::empty()).await?;
    sh_ok(&long_sandbox, &opened.box_id, "printf x > /work/x").await?;
    store_gate.set(GateMode::DenyManifestWrite);
    let saved = long_sandbox.save_box(&opened.box_id).await;
    store_gate.set(GateMode::Open);
    adapter::release(&long_sandbox, &opened.box_id).await?;
    battery::common::settle().await?;
    checks.expect(
        "start-save-failed-store-refusal-is-a-store-error",
        matches!(saved, Err(Error::Store(_))),
        format!("{saved:?}"),
    );
    let (start, _) = start_of(&long_sandbox, &lane, Some(k1)).await?;
    let failed = fresh(start);
    checks.expect(
        "start-save-failed-after-a-refused-manifest",
        failed == Some(FreshReason::SaveFailed { turn: k1 }),
        format!("{failed:?}"),
    );
    let lane2 = Lane::new()?;
    let k9 = ids::turn()?;
    let key9 = saves.key(lane2.thread, lane2.agent, k9);
    let started = Record::started(AttemptId::mint(), Timestamp::now());
    saves
        .store
        .put(
            &record_key(&key9),
            Bytes::from(
                started
                    .to_vec()
                    .map_err(|e| battery::error::data("record", e.to_string()))?,
            ),
        )
        .await?;
    let (start, _) = start_of(&long_sandbox, &lane2, Some(k9)).await?;
    checks.expect(
        "start-save-failed-when-the-record-is-started-with-no-manifest",
        fresh(start) == Some(FreshReason::SaveFailed { turn: k9 }),
        format!("{start:?}"),
    );
    if let Some(reason) = failed {
        results.push(("SaveFailed", reason));
    }

    for (name, delete_only) in [("overwritten-part", false), ("deleted-part", true)] {
        let lane = Lane::new()?;
        let (k1, _) = state_turn(&long_sandbox, &lane, None, 53, 5_000_000).await?;
        let key = saves.key(lane.thread, lane.agent, k1);
        let part = saves.part_of(&key).await?;
        if delete_only {
            saves.store.delete(&part).await?;
        } else {
            saves
                .store
                .put(&part, Bytes::from(Prng::new(5).bytes(2_000_000)))
                .await?;
        }
        let (start, pod) = start_of(&long_sandbox, &lane, Some(k1)).await?;
        let reason = fresh(start);
        checks.expect(
            &format!("start-restore-failed-{name}"),
            reason == Some(FreshReason::RestoreFailed { turn: k1 }),
            format!("{reason:?}"),
        );
        if let Some(reason) = reason
            && !delete_only
        {
            results.push(("RestoreFailed", reason));
        }
        let _ = pod;
        let thread = lane.thread.to_string();
        let live = pool::pods_of_thread(env, &thread).await?;
        let mut leftover = 0;
        for pod in live {
            leftover += usize::from(
                pool::pod(env, &pod)
                    .await?
                    .is_some_and(|p| p["metadata"]["deletionTimestamp"].is_null()),
            );
        }
        checks.expect(
            &format!("start-restore-failed-{name}-leaves-no-live-pod-of-the-thread"),
            leftover == 0,
            format!("{leftover} live pods"),
        );
    }

    let lane = Lane::new()?;
    let (k1, _) = state_turn(&long_sandbox, &lane, None, 54, 1000).await?;
    store_gate.set(GateMode::Refuse);
    let outcome = long_sandbox
        .open_box(&request(lane.child(k1)?.1, Seed::empty(), None)?)
        .await;
    store_gate.set(GateMode::Open);
    let unavailable = outcome.as_ref().ok().and_then(|o| fresh(o.start));
    checks.expect(
        "start-store-unavailable-is-ok-and-fresh-not-an-error",
        unavailable == Some(FreshReason::StoreUnavailable),
        format!("{:?}", outcome.as_ref().map(|o| o.start)),
    );
    if let Ok(opened) = outcome {
        adapter::release(&long_sandbox, &opened.box_id).await?;
        battery::common::settle().await?;
    }
    if let Some(reason) = unavailable {
        results.push(("StoreUnavailable", reason));
    }

    let lane = Lane::new()?;
    let (k1, _) = state_turn(&long_sandbox, &lane, None, 55, 1000).await?;
    let (start, _) = start_of(&long_sandbox, &lane, Some(k1)).await?;
    checks.expect(
        "start-rollback-is-restored",
        restored(&start, k1),
        format!("{start:?}"),
    );
    long_sandbox.erase(lane.thread).await?;
    let (start, _) = start_of(&long_sandbox, &lane, Some(k1)).await?;
    let objects = saves.objects(lane.thread).await?;
    checks.expect(
        "start-thread-delete-gives-no-save-ever-and-empty-prefixes",
        fresh(start) == Some(FreshReason::NoSaveEver) && objects.total() == 0,
        format!("{start:?}, {} objects", objects.total()),
    );
    let _ = manifest_key;
    env.record(
        "record_lifetime_scenario",
        "save.tombstone_ttl_days (days only; not tested through the chart)",
        "text",
    );

    let distinct = results.iter().enumerate().all(|(i, (_, a))| {
        results
            .iter()
            .skip(i + 1)
            .all(|(_, b)| discriminant(a) != discriminant(b))
    });
    checks.expect(
        "start-results-are-pairwise-different",
        distinct && results.len() == 5,
        format!("{:?}", results.iter().map(|(n, _)| *n).collect::<Vec<_>>()),
    );
    checks.finish()
}
