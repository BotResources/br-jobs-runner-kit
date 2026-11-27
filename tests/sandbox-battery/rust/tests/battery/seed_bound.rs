use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter::{self, Tunables};
use battery::common::ensure_floor;
use battery::env::Env;
use battery::error::Res;
use battery::evidence::Checks;
use battery::guards::ValueGuard;
use battery::ids::Lane;
use battery::{pool, seeds};

const GONE_SECONDS: u64 = 90;
const VERSIONS: &str = "adapter 0.1.2, chart 0.2.4, keeper 0.1.1";

struct Try {
    seconds: f64,
    outer_fired: bool,
    result: String,
    port_fault: bool,
    seed_written: Option<bool>,
    pods: Vec<String>,
    gone: bool,
}

async fn stress_open(env: &Env, sandbox: &br_runner_sandbox::Sandbox, outer: Duration) -> Res<Try> {
    let stress = seeds::stress()?;
    let lane = Lane::new()?;
    let thread = lane.thread.to_string();
    let seen = Arc::new(Mutex::new(BTreeSet::<String>::new()));
    let watcher = {
        let seen = Arc::clone(&seen);
        let env = env.clone();
        let thread = thread.clone();
        tokio::spawn(async move {
            loop {
                if let Ok(names) = pool::pods_of_thread(&env, &thread).await {
                    seen.lock().unwrap().extend(names);
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        })
    };
    let started = Instant::now();
    let outcome = tokio::time::timeout(
        outer,
        adapter::open(sandbox, lane.first()?.1, stress.seed.clone()),
    )
    .await;
    let seconds = started.elapsed().as_secs_f64();
    watcher.abort();
    let last = pool::pods_of_thread(env, &thread).await?;
    seen.lock().unwrap().extend(last);
    let outer_fired = outcome.is_err();
    let result = match &outcome {
        Err(_) => format!(
            "outer bound of {} s fired, the open did not return",
            outer.as_secs()
        ),
        Ok(Ok(opened)) => format!("Ok, box {}", opened.box_id),
        Ok(Err(error)) => format!("Err({error})"),
    };
    let port_fault =
        matches!(&outcome, Ok(Err(error)) if error.to_string().starts_with("the port gave"));
    let mut seed_written = None;
    if let Ok(Ok(opened)) = &outcome {
        seen.lock().unwrap().insert(opened.box_id.to_string());
        seed_written =
            Some(seeds::present(sandbox, &opened.box_id, &env.seed_root, &stress).await?);
        adapter::release(sandbox, &opened.box_id).await?;
    }
    let pods: Vec<String> = seen.lock().unwrap().iter().cloned().collect();
    if outer_fired {
        for name in &pods {
            pool::delete_now(env, name).await?;
        }
    }
    let mut gone = !pods.is_empty();
    for name in &pods {
        gone &= pool::wait_gone(env, name, GONE_SECONDS).await.is_ok();
    }
    Ok(Try {
        seconds,
        outer_fired,
        result,
        port_fault,
        seed_written,
        pods,
        gone,
    })
}

fn record(env: &Env, label: &str, attempt: usize, done: &Try) {
    let seed = match done.seed_written {
        Some(true) => "yes",
        Some(false) => "no",
        None => "not checked",
    };
    env.record(
        &format!("stress_seed_open_{label}_try_{attempt}"),
        format!(
            "{:.1} s; result {}; seed files written and equal {seed}; pod {} gone {}; outer bound fired {}",
            done.seconds,
            done.result,
            done.pods.join(","),
            if done.gone { "yes" } else { "no" },
            if done.outer_fired { "yes" } else { "no" }
        ),
        &format!("event; {VERSIONS}"),
    );
}

async fn series(label: &str, exec_timeout: Option<u64>, tries: usize, outer: u64) -> Res<()> {
    let session = Session::start().await?;
    let env = &session.env;
    let mut values = ValueGuard::new(env);
    ensure_floor(env, &mut values, 3).await?;
    let mut checks = Checks::new(env);
    let mut tunables = Tunables::standard(env);
    tunables.exec_timeout = exec_timeout;
    let sandbox = tunables.connect(env).await?;
    for attempt in 1..=tries {
        pool::wait_warm(env, 3, 180).await?;
        let done = stress_open(env, &sandbox, Duration::from_secs(outer)).await?;
        record(env, label, attempt, &done);
        if exec_timeout.is_some() {
            let ended = !done.outer_fired && (done.port_fault || done.seed_written == Some(true));
            checks.expect(
                &format!("stress-seed-open-try-{attempt}-ends-within-the-outer-bound-with-the-seed-written-or-a-port-fault"),
                ended,
                format!("{:.1} s, {}, seed written {:?}", done.seconds, done.result, done.seed_written),
            );
        }
        checks.expect(
            &format!("stress-seed-open-try-{attempt}-leaves-no-pod-of-the-thread"),
            done.gone,
            done.pods.join(","),
        );
    }
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn open_with_the_stress_seed_and_a_set_bound_ends_within_the_outer_bound_and_leaves_no_pod()
-> Res {
    series("exec_timeout_120", Some(120), 3, 300).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn open_with_the_stress_seed_and_no_bound_leaves_no_pod_after_it_ends_or_is_stopped() -> Res {
    series("no_bound", None, 2, 150).await
}
