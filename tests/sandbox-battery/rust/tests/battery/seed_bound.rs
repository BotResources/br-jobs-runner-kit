use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter::{self, Tunables};
use battery::common::ensure_floor;
use battery::error::Res;
use battery::evidence::Checks;
use battery::guards::ValueGuard;
use battery::ids::Lane;
use battery::{pool, seeds};

const TRIES: usize = 3;
const OUTER_BOUND: Duration = Duration::from_secs(300);
const SET_BOUND_SECONDS: u64 = 120;
const GONE_SECONDS: u64 = 90;
const VERSIONS: &str = "adapter 0.1.2, chart 0.2.4, keeper 0.1.1";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn open_with_the_stress_seed_and_a_set_bound_ends_with_a_port_fault_and_deletes_the_pod()
-> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut values = ValueGuard::new(env);
    ensure_floor(env, &mut values, 3).await?;
    let mut checks = Checks::new(env);
    let mut tunables = Tunables::standard(env);
    tunables.exec_timeout = Some(SET_BOUND_SECONDS);
    let sandbox = tunables.connect(env).await?;
    let stress = seeds::stress()?;
    for attempt in 1..=TRIES {
        pool::wait_warm(env, 3, 180).await?;
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
            OUTER_BOUND,
            adapter::open(&sandbox, lane.first()?.1, stress.seed.clone()),
        )
        .await;
        let seconds = started.elapsed().as_secs_f64();
        watcher.abort();
        let last = pool::pods_of_thread(env, &thread).await?;
        seen.lock().unwrap().extend(last);
        let outer_fired = outcome.is_err();
        let result = match &outcome {
            Err(_) => "outer bound of 300 s fired, the open did not return".to_owned(),
            Ok(Ok(opened)) => format!("Ok, box {}", opened.box_id),
            Ok(Err(error)) => format!("Err({error})"),
        };
        let mut seed_written = "not checked";
        if let Ok(Ok(opened)) = &outcome {
            seen.lock().unwrap().insert(opened.box_id.to_string());
            seed_written =
                if seeds::present(&sandbox, &opened.box_id, &env.seed_root, &stress).await? {
                    "yes"
                } else {
                    "no"
                };
            adapter::release(&sandbox, &opened.box_id).await?;
        }
        let names: Vec<String> = seen.lock().unwrap().iter().cloned().collect();
        if outer_fired {
            for name in &names {
                pool::delete_now(env, name).await?;
            }
        }
        let mut gone = !names.is_empty();
        for name in &names {
            gone &= pool::wait_gone(env, name, GONE_SECONDS).await.is_ok();
        }
        let port_fault =
            matches!(&outcome, Ok(Err(error)) if error.to_string().starts_with("the port gave"));
        env.record(
            &format!("stress_seed_open_exec_timeout_{SET_BOUND_SECONDS}_try_{attempt}"),
            format!(
                "{seconds:.1} s; result {result}; seed files written and equal {seed_written}; pod {} gone after release {}; outer bound fired {}",
                names.join(","),
                if gone { "yes" } else { "no" },
                if outer_fired { "yes" } else { "no" }
            ),
            &format!("event; {VERSIONS}"),
        );
        checks.expect(
            &format!(
                "stress-seed-open-try-{attempt}-ends-with-a-port-fault-before-the-outer-bound"
            ),
            port_fault && !outer_fired,
            format!("{seconds:.1} s, {result}"),
        );
        checks.expect(
            &format!("stress-seed-open-try-{attempt}-leaves-no-pod-of-the-thread"),
            gone,
            names.join(","),
        );
    }
    checks.finish()
}
