use battery::Session;
use battery::adapter::{self, sh_ok};
use battery::common::{restored, work_hash};
use battery::error::Res;
use battery::evidence::Checks;
use battery::ids::{self, Lane};
use battery::saves::Saves;
use br_agentic_harness_contract::{FreshReason, Seed, StartKind, TurnId};
use br_runner_sandbox::Sandbox;

async fn run_turn(
    sandbox: &Sandbox,
    lane: &Lane,
    id: TurnId,
    parent: Option<TurnId>,
    file: &str,
) -> Res<String> {
    let opened = adapter::open(sandbox, lane.turn(id, parent), Seed::empty()).await?;
    sh_ok(
        sandbox,
        &opened.box_id,
        &format!("printf '{file}' > /work/{file}"),
    )
    .await?;
    let hash = work_hash(sandbox, &opened.box_id).await?;
    adapter::save(sandbox, &opened.box_id).await?;
    adapter::release(sandbox, &opened.box_id).await?;
    battery::common::settle().await?;
    Ok(hash)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn go_back_to_turn_one_per_agent_by_turn_id() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let first = Lane::new()?;
    let second = first.with_agent()?;
    let lanes = [("a1", first, second), ("a2", second, first)];
    for (name, lane, other_lane) in &lanes {
        let k2 = ids::turn()?;
        let k1 = ids::turn()?;
        let k3 = ids::turn()?;
        let h1 = run_turn(&sandbox, lane, k1, None, &format!("{name}-one")).await?;
        let h2 = run_turn(&sandbox, lane, k2, Some(k1), &format!("{name}-two")).await?;
        let h3 = run_turn(&sandbox, lane, k3, Some(k2), &format!("{name}-three")).await?;
        checks.expect(
            &format!("go-back-{name}-three-different-trees"),
            h1 != h2 && h2 != h3,
            "hashes",
        );
        let back =
            adapter::open(&sandbox, lane.turn(ids::turn()?, Some(k1)), Seed::empty()).await?;
        let h_back = work_hash(&sandbox, &back.box_id).await?;
        let listing = sh_ok(&sandbox, &back.box_id, "ls /work").await?;
        checks.expect(
            &format!("go-back-{name}-restored-k1-with-the-tree-of-k1"),
            restored(&back.start, k1) && h_back == h1,
            format!("{:?} {listing}", back.start),
        );
        checks.expect(
            &format!("go-back-{name}-no-file-of-k2-or-k3"),
            !listing.contains("two") && !listing.contains("three"),
            listing,
        );
        adapter::release(&sandbox, &back.box_id).await?;
        battery::common::settle().await?;
        let k4 = ids::turn()?;
        let h4 = run_turn(&sandbox, lane, k4, Some(k1), &format!("{name}-four")).await?;
        let after =
            adapter::open(&sandbox, lane.turn(ids::turn()?, Some(k4)), Seed::empty()).await?;
        let listing = sh_ok(&sandbox, &after.box_id, "ls /work").await?;
        let h_after = work_hash(&sandbox, &after.box_id).await?;
        checks.expect(
            &format!("go-back-{name}-new-turn-from-k1-holds-k1-plus-its-own-file"),
            restored(&after.start, k4)
                && h_after == h4
                && listing.contains("four")
                && listing.contains("one")
                && !listing.contains("two")
                && !listing.contains("three"),
            listing.clone(),
        );
        let foreign = adapter::open(
            &sandbox,
            other_lane.turn(ids::turn()?, Some(k1)),
            Seed::empty(),
        )
        .await?;
        let foreign_listing = sh_ok(&sandbox, &foreign.box_id, "ls /work").await?;
        checks.expect(
            &format!("go-back-{name}-parent-of-the-other-agent-is-never-restored"),
            foreign.start == StartKind::Fresh(FreshReason::NoSaveEver)
                && foreign_listing.trim().is_empty(),
            format!("{:?} {foreign_listing:?}", foreign.start),
        );
        adapter::release(&sandbox, &foreign.box_id).await?;
        battery::common::settle().await?;
        adapter::release(&sandbox, &after.box_id).await?;
        battery::common::settle().await?;
        let still = saves
            .manifest(&saves.key(lane.thread, lane.agent, k2))
            .await?
            .is_some()
            && saves
                .manifest(&saves.key(lane.thread, lane.agent, k3))
                .await?
                .is_some();
        checks.expect(
            &format!("go-back-{name}-saves-of-k2-and-k3-still-exist"),
            still,
            "manifests",
        );
    }
    checks.finish()
}
