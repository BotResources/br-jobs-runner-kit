use std::time::Duration;

use battery::Session;
use battery::adapter::{self, Tunables, request};
use battery::common::{empty_pool, wait_until};
use battery::error::Res;
use battery::evidence::Checks;
use battery::guards::PodReaper;
use battery::ids::Lane;
use battery::pool;
use battery::saves::Saves;
use br_agentic_harness_contract::Seed;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_open_leaves_no_pod_and_no_instance() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let keeper = empty_pool(env).await?;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (_, turn) = lane.first()?;
    let req = request(turn, Seed::empty(), None)?;
    let runner = sandbox.clone();
    let again = req.clone();
    let first =
        tokio::spawn(async move { runner.open_box(&again).await.map(|o| o.box_id.to_string()) });
    let thread = lane.thread.to_string();
    wait_until("the cold pod to exist", 30, || async {
        Ok(!pool::pods_of_thread(env, &thread).await?.is_empty())
    })
    .await?;
    let first_pod = pool::pods_of_thread(env, &thread).await?;
    first.abort();
    let _ = first.await;
    let second = tokio::time::timeout(Duration::from_secs(60), sandbox.open_box(&req)).await;
    checks.expect(
        "dropped-open-lets-the-same-instance-open-at-once",
        matches!(&second, Ok(Ok(_))),
        format!("{second:?}"),
    );
    if let Ok(Ok(opened)) = second {
        adapter::release(&sandbox, &opened.box_id).await?;
    }
    let gone = wait_until("the pod of the dropped open to be deleted", 60, || async {
        let mut all_gone = true;
        for name in &first_pod {
            all_gone &= pool::pod(env, name)
                .await?
                .is_none_or(|p| !p["metadata"]["deletionTimestamp"].is_null());
        }
        Ok(all_gone)
    })
    .await;
    checks.expect(
        "dropped-open-pod-is-deleted-by-the-runtime-task",
        gone.is_ok(),
        format!("{gone:?} for {first_pod:?}"),
    );
    drop(keeper);
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_erase_of_a_thread_leaves_nothing_in_the_store_and_the_cluster() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut tunables = Tunables::standard(env);
    tunables.idle_keep_min = 1;
    let sandbox = tunables.connect(env).await?;
    let saves = Saves::through(env, &tunables)?;
    let mut checks = Checks::new(env);
    let mut reaper = PodReaper::new(env);
    let lane = Lane::new()?;
    let mut parent = None;
    for round in 0..3 {
        let (id, turn) = match parent {
            Some(p) => lane.child(p)?,
            None => lane.first()?,
        };
        let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
        reaper.track(opened.box_id.to_string());
        adapter::sh_ok(
            &sandbox,
            &opened.box_id,
            &format!("printf {round} > /work/turn"),
        )
        .await?;
        adapter::save(&sandbox, &opened.box_id).await?;
        adapter::release(&sandbox, &opened.box_id).await?;
        parent = Some(id);
    }
    let thread = lane.thread.to_string();
    let before = saves.objects(lane.thread).await?;
    let pods_before = pool::pods_of_thread(env, &thread).await?.len();
    checks.expect(
        "erase-case-has-objects-and-a-kept-pod",
        before.total() >= 9 && pods_before >= 1,
        format!("{} objects, {pods_before} pods", before.total()),
    );
    sandbox.erase(lane.thread).await?;
    let second = sandbox.erase(lane.thread).await;
    let after = saves.objects(lane.thread).await?;
    wait_until("the thread pods to be gone", 60, || async {
        let mut live = 0;
        for name in pool::pods_of_thread(env, &thread).await? {
            live += usize::from(
                pool::pod(env, &name)
                    .await?
                    .is_some_and(|p| p["metadata"]["deletionTimestamp"].is_null()),
            );
        }
        Ok(live == 0)
    })
    .await?;
    checks.expect(
        "erase-leaves-no-object",
        after.total() == 0,
        format!("{} left", after.total()),
    );
    checks.expect(
        "second-erase-is-a-success",
        second.is_ok(),
        format!("{second:?}"),
    );
    checks.finish()
}
