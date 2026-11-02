mod support;

use std::sync::{Arc, Mutex};

use br_graph_contract::CancelView;
use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_toolkit::{RunRefusal, StartArgs, StartedRun, run_start};
use support::*;

#[tokio::test(start_paused = true)]
async fn given_a_run_whose_start_function_panics_before_it_returns_its_future_when_the_run_fails_with_panic_then_the_cancel_token_of_the_run_is_triggered()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let seen: Arc<Mutex<Option<CancelView>>> = Arc::default();
    let keep = Arc::clone(&seen);
    let panicking = StartArgs::new(
        single(Behavior::Result("x")),
        run_start(move |input| {
            *keep.lock().expect("lock") = Some(input.cancel.clone());
            if true {
                panic!("the function panics before it returns its future");
            }
            async {
                Ok::<StartedRun<Flow>, RunRefusal>(StartedRun::new(Flow::default(), Vec::new()))
            }
        }),
        false,
    );
    let running = run(&jobs, |b| b, 1, panicking);
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(failed_fact(&jobs).expect("failed").reason_code, "panic");
    let view = seen.lock().expect("lock").take().expect("the view");
    assert!(view.is_triggered());
}
