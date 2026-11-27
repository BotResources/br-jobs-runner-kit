use std::sync::Arc;
use std::time::Duration;

use battery::Session;
use battery::adapter::{self, request, sh_ok};
use battery::common::{empty_pool, wait_until};
use battery::error::Res;
use battery::evidence::Checks;
use battery::ids::Lane;
use battery::pool;
use battery::saves::Saves;
use battery::seeds;
use battery::world::HarnessWorld;
use br_agentic_contract::Site;
use br_agentic_harness::RunEnd;
use br_agentic_harness_contract::{SandboxPort, Seed, WebAccess};
use br_runner_sandbox::Error;

const CANARY: &str = "canary-7f3a91c2d84e";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_instance_with_an_open_box_is_refused_until_the_box_is_released() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let world = HarnessWorld::new(sandbox.clone(), None)?;
    let (_, turn) = world.first()?;
    world.open(turn, Seed::empty()).await?;
    let (_, second_turn) = world.first()?;
    let refused = world.open(second_turn, Seed::empty()).await;
    checks.expect(
        "harness-second-open-of-an-open-instance-is-a-refusal",
        matches!(&refused, Err(error) if error.is_refusal()),
        format!("{refused:?}"),
    );
    checks.expect(
        "harness-refused-open-never-reaches-the-sandbox",
        world.spy.steps() == ["open"],
        format!("{:?}", world.spy.steps()),
    );
    world.close(RunEnd::Failed).await?;
    battery::common::settle().await?;
    let (_, third_turn) = world.first()?;
    let third = world.open(third_turn, Seed::empty()).await;
    checks.expect(
        "harness-open-after-close-is-accepted",
        third.is_ok(),
        format!("{third:?}"),
    );
    world.close(RunEnd::Failed).await?;
    battery::common::settle().await?;

    let lane = Lane::new()?;
    let (_, direct_turn) = lane.first()?;
    let direct = request(direct_turn, Seed::empty(), None)?;
    let first = sandbox.open_box(&direct).await?;
    let again = sandbox.open_box(&direct).await;
    checks.expect(
        "adapter-second-open-of-an-open-instance-is-instance-open",
        matches!(&again, Err(Error::InstanceOpen(_))),
        format!("{again:?}"),
    );
    adapter::release(&sandbox, &first.box_id).await?;
    battery::common::settle().await?;
    let after = sandbox.open_box(&direct).await;
    checks.expect(
        "adapter-open-after-release-is-accepted",
        after.is_ok(),
        format!("{after:?}"),
    );
    if let Ok(opened) = after {
        adapter::release(&sandbox, &opened.box_id).await?;
        battery::common::settle().await?;
    }
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_harness_open_that_is_dropped_leaves_no_pod_and_no_instance() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let keeper = empty_pool(env).await?;
    let mut checks = Checks::new(env);
    let world = Arc::new(HarnessWorld::new(adapter::standard(env).await?, None)?);
    let (_, turn) = world.first()?;
    let runner = world.clone();
    let pending = tokio::spawn(async move { runner.open(turn, Seed::empty()).await });
    let thread = world.lane.thread.to_string();
    wait_until("the cold pod to exist", 30, || async {
        Ok(!pool::pods_of_thread(env, &thread).await?.is_empty())
    })
    .await?;
    let pods = pool::pods_of_thread(env, &thread).await?;
    pending.abort();
    let _ = pending.await;
    let (_, next) = world.first()?;
    let second =
        tokio::time::timeout(Duration::from_secs(60), world.open(next, Seed::empty())).await;
    checks.expect(
        "harness-dropped-open-lets-the-same-instance-open-again",
        matches!(&second, Ok(Ok(_))),
        format!("{second:?}"),
    );
    world.close(RunEnd::Failed).await?;
    let gone = wait_until("the pod of the dropped open to be deleted", 60, || async {
        let mut all = true;
        for name in &pods {
            all &= pool::pod(env, name)
                .await?
                .is_none_or(|pod| !pod["metadata"]["deletionTimestamp"].is_null());
        }
        Ok(all)
    })
    .await;
    checks.expect(
        "harness-dropped-open-pod-is-deleted",
        gone.is_ok(),
        format!("{gone:?} for {pods:?}"),
    );
    drop(keeper);
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_seed_given_to_the_harness_open_reaches_the_box() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut checks = Checks::new(env);
    let world = HarnessWorld::new(adapter::standard(env).await?, None)?;
    let set = seeds::make(21, 12, 50_000, 49_000)?;
    let (_, turn) = world.first()?;
    world.open(turn, set.seed.clone()).await?;
    let boxed = world.boxed()?;
    let present = seeds::present(world.spy.sandbox(), &boxed, &env.seed_root, &set).await?;
    checks.expect(
        "harness-open-seed-files-are-in-the-box-with-their-hashes",
        present,
        "the seed files differ",
    );
    world.close(RunEnd::Failed).await?;
    battery::common::settle().await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn web_access_that_reaches_nothing_opens_and_web_access_that_reaches_a_site_is_refused_in_mode_none()
-> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut checks = Checks::new(env);
    let accepted = [
        ("none-setting", None),
        ("web-none", Some(WebAccess::None)),
        ("empty-whitelist", Some(WebAccess::Whitelist(Vec::new()))),
    ];
    for (name, web) in accepted {
        let world = HarnessWorld::new(adapter::standard(env).await?, web)?;
        let (_, turn) = world.first()?;
        let opened = world.open(turn, Seed::empty()).await;
        checks.expect(
            &format!("harness-mode-none-accepts-{name}"),
            opened.is_ok(),
            format!("{opened:?}"),
        );
        world.close(RunEnd::Failed).await?;
        battery::common::settle().await?;
    }
    let refused = [
        ("full", WebAccess::Full),
        (
            "site-list",
            WebAccess::Whitelist(vec![
                Site::new("docs.example.org")
                    .map_err(|e| battery::error::data("site", e.to_string()))?,
            ]),
        ),
    ];
    for (name, web) in refused {
        let world = HarnessWorld::new(adapter::standard(env).await?, Some(web))?;
        let (_, turn) = world.first()?;
        let opened = world.open(turn, Seed::empty()).await;
        let pods = pool::pods_of_thread(env, &world.lane.thread.to_string()).await?;
        checks.expect(
            &format!("harness-mode-none-refuses-{name}-and-makes-no-pod"),
            opened.is_err() && pods.is_empty(),
            format!("{opened:?} pods {pods:?}"),
        );
    }
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_canary_in_a_seed_file_a_write_and_a_command_stays_out_of_the_pod_metadata_and_the_save_record()
-> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let world = HarnessWorld::new(adapter::standard(env).await?, None)?;
    let seed = Seed::new(vec![br_agentic_harness_contract::SeedEntry::file(
        adapter::path("canary.txt")?,
        0o644,
        CANARY.as_bytes().to_vec(),
    )])
    .map_err(|e| battery::error::data("seed", e.to_string()))?;
    let (id, turn) = world.first()?;
    world.open(turn, seed).await?;
    let boxed = world.boxed()?;
    let sandbox = world.spy.sandbox();
    SandboxPort::write(
        &world.spy,
        &boxed,
        &adapter::path("work/written.txt")?,
        CANARY.as_bytes().to_vec(),
    )
    .await?;
    sh_ok(
        sandbox,
        &boxed,
        &format!("echo {CANARY} > /work/by-command.txt"),
    )
    .await?;
    let pod = pool::pod(env, &boxed.to_string())
        .await?
        .ok_or_else(|| battery::error::data("the pod", "it is gone"))?;
    checks.expect(
        "canary-is-not-in-the-pod-labels-annotations-or-spec",
        !pod.to_string().contains(CANARY),
        "the canary is in the pod object",
    );
    world.close(RunEnd::Completed).await?;
    battery::common::settle().await?;
    let key = saves.key(world.lane.thread, world.lane.agent, id);
    let manifest = saves.manifest(&key).await?;
    checks.expect(
        "canary-is-not-in-the-manifest",
        manifest.is_some_and(|m| !format!("{m:?}").contains(CANARY)),
        "the canary is in the manifest or no manifest",
    );
    let objects = saves.objects(world.lane.thread).await?;
    let names = format!(
        "{:?} {:?} {:?}",
        objects.saves, objects.records, objects.tombstones
    );
    checks.expect(
        "canary-is-not-in-any-object-name",
        !names.contains(CANARY),
        names,
    );
    checks.finish()
}
