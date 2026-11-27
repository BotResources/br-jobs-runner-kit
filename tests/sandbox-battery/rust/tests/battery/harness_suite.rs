use battery::Session;
use battery::adapter::{self, sh_ok};
use battery::common::{restored, work_hash};
use battery::error::Res;
use battery::evidence::Checks;
use battery::pool;
use battery::saves::Saves;
use battery::tree;
use battery::world::HarnessWorld;
use br_agentic_harness::RunEnd;
use br_agentic_harness_contract::{
    AlreadySaved, BoxId, FreshReason, ProcessId, SandboxPort, Seed, StartKind, TurnId,
};

const ENDS: [(RunEnd, bool); 3] = [
    (RunEnd::Completed, true),
    (RunEnd::Failed, false),
    (RunEnd::Cancelled, false),
];

async fn open_with_state(world: &HarnessWorld, seed: u64) -> Res<(TurnId, BoxId, StartKind)> {
    let (id, turn) = world.first()?;
    let start = world.open(turn, Seed::empty()).await?;
    let boxed = world.boxed()?;
    tree::state(world.spy.sandbox(), &boxed, seed, 200_000).await?;
    Ok((id, boxed, start))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_completed_close_stops_the_commands_then_saves_then_releases_and_the_next_turn_restores()
-> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let world = HarnessWorld::new(adapter::standard(env).await?, None)?;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let (first, boxed, start) = open_with_state(&world, 71).await?;
    checks.expect(
        "harness-first-open-is-fresh-with-no-save-ever",
        start == StartKind::Fresh(FreshReason::NoSaveEver),
        format!("{start:?}"),
    );
    let hash = work_hash(world.spy.sandbox(), &boxed).await?;
    for _ in 0..2 {
        let process = adapter::background(world.spy.sandbox(), &boxed, "sleep 600").await?;
        world.track(process)?;
    }
    world.close(RunEnd::Completed).await?;
    battery::common::settle().await?;
    checks.expect(
        "harness-completed-close-order-is-open-kill-kill-save-release",
        world.spy.steps() == ["open", "kill", "kill", "save", "release"],
        format!("{:?}", world.spy.steps()),
    );
    let key = saves.key(world.lane.thread, world.lane.agent, first);
    checks.expect(
        "harness-completed-close-leaves-a-manifest",
        saves.manifest(&key).await?.is_some(),
        "no manifest",
    );
    let gone = pool::wait_gone(env, &boxed.to_string(), 60).await;
    checks.expect(
        "harness-completed-close-deletes-the-pod-and-its-commands",
        gone.is_ok(),
        format!("{gone:?}"),
    );
    let (_, next) = world.child(first)?;
    let again = world.open(next, Seed::empty()).await?;
    let second = world.boxed()?;
    checks.expect(
        "harness-next-turn-is-restored",
        restored(&again, first),
        format!("{again:?}"),
    );
    checks.expect(
        "harness-next-turn-gets-the-saved-files",
        work_hash(world.spy.sandbox(), &second).await? == hash,
        "hash differs",
    );
    world.close(RunEnd::Failed).await?;
    battery::common::settle().await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_or_cancelled_close_stops_the_commands_and_releases_and_never_saves() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    for (end, _) in ENDS.iter().filter(|(_, saved)| !saved) {
        let world = HarnessWorld::new(adapter::standard(env).await?, None)?;
        let (first, boxed, _) = open_with_state(&world, 72).await?;
        for _ in 0..2 {
            let process = adapter::background(world.spy.sandbox(), &boxed, "sleep 600").await?;
            world.track(process)?;
        }
        world.close(*end).await?;
        battery::common::settle().await?;
        let name = format!("{end:?}").to_lowercase();
        checks.expect(
            &format!("harness-{name}-close-order-is-open-kill-kill-release"),
            world.spy.steps() == ["open", "kill", "kill", "release"],
            format!("{:?}", world.spy.steps()),
        );
        let key = saves.key(world.lane.thread, world.lane.agent, first);
        checks.expect(
            &format!("harness-{name}-close-leaves-no-manifest"),
            saves.manifest(&key).await?.is_none(),
            "a manifest exists",
        );
        let (_, next) = world.child(first)?;
        let again = world.open(next, Seed::empty()).await?;
        checks.expect(
            &format!("harness-{name}-close-leaves-the-next-turn-fresh"),
            again == StartKind::Fresh(FreshReason::NoSaveEver),
            format!("{again:?}"),
        );
        world.close(RunEnd::Failed).await?;
        battery::common::settle().await?;
    }
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_process_that_cannot_be_killed_does_not_stop_the_save_and_the_release() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    for (end, saved) in ENDS {
        let world = HarnessWorld::new(adapter::standard(env).await?, None)?;
        let (first, _, _) = open_with_state(&world, 73).await?;
        world.track(ProcessId::new("999999"))?;
        let closed = world.close(end).await;
        let name = format!("{end:?}").to_lowercase();
        let expected: Vec<&str> = if saved {
            vec!["open", "kill", "save", "release"]
        } else {
            vec!["open", "kill", "release"]
        };
        checks.expect(
            &format!("harness-unkillable-{name}-close-still-saves-as-needed-and-releases"),
            world.spy.steps() == expected,
            format!("{:?} close gave {closed:?}", world.spy.steps()),
        );
        let key = saves.key(world.lane.thread, world.lane.agent, first);
        checks.expect(
            &format!("harness-unkillable-{name}-save-matches-the-end"),
            saves.manifest(&key).await?.is_some() == saved,
            "manifest state",
        );
    }
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_box_already_saved_by_its_own_turn_does_not_fail_the_completed_close() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let world = HarnessWorld::new(adapter::standard(env).await?, None)?;
    let (first, boxed, _) = open_with_state(&world, 74).await?;
    let direct = SandboxPort::save(&world.spy, &boxed).await;
    let again = SandboxPort::save(&world.spy, &boxed).await;
    checks.expect(
        "harness-first-direct-save-works",
        direct.is_ok(),
        format!("{direct:?}"),
    );
    checks.expect(
        "harness-second-save-of-one-box-is-already-saved",
        matches!(&again, Err(error) if AlreadySaved::is(error)),
        format!("{again:?}"),
    );
    let closed = world.close(RunEnd::Completed).await;
    checks.expect(
        "harness-completed-close-tolerates-already-saved",
        closed.is_ok(),
        format!("{closed:?}"),
    );
    checks.expect(
        "harness-close-after-save-still-releases",
        world.spy.steps().last() == Some(&"release"),
        format!("{:?}", world.spy.steps()),
    );
    let key = saves.key(world.lane.thread, world.lane.agent, first);
    checks.expect(
        "harness-one-manifest-after-the-double-save",
        saves.manifest(&key).await?.is_some(),
        "no manifest",
    );
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_close_of_an_instance_with_no_open_box_is_a_success_and_touches_nothing() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut checks = Checks::new(env);
    let world = HarnessWorld::new(adapter::standard(env).await?, None)?;
    let never = world.close(RunEnd::Completed).await;
    let (_, _, _) = open_with_state(&world, 75).await?;
    world.close(RunEnd::Failed).await?;
    battery::common::settle().await?;
    let twice = world.close(RunEnd::Failed).await;
    let steps = world.spy.steps();
    checks.expect(
        "harness-close-with-no-open-box-succeeds",
        never.is_ok() && twice.is_ok(),
        format!("{never:?} {twice:?}"),
    );
    checks.expect(
        "harness-close-with-no-open-box-calls-nothing",
        steps == ["open", "release"],
        format!("{steps:?}"),
    );
    let leftover = sh_ok(world.spy.sandbox(), &world.boxed()?, "true").await;
    checks.expect(
        "harness-released-box-is-not-usable",
        leftover.is_err(),
        format!("{leftover:?}"),
    );
    checks.finish()
}
