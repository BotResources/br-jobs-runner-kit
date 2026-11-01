mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use br_runner_jobs::Error;
use br_runner_jobs::test_double::{Delivery, ScriptedJobs};
use br_runner_toolkit::RunnerBuilder;
use support::*;
use tokio_util::sync::CancellationToken;

#[tokio::test(start_paused = true)]
async fn given_a_write_queued_before_the_executor_returned_that_jobs_keeps_refusing_with_a_resendable_error_when_the_graph_ends_then_the_slot_stays_held_and_no_terminal_fact_is_sent_until_the_write_goes_through()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    jobs.push_trigger(trigger(2));
    let open = Arc::new(AtomicBool::new(false));
    let switch = Arc::clone(&open);
    jobs.answer_register(move || {
        if switch.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(Error::Unreadable("the answer of jobs".to_owned()))
        }
    });
    let builder = RunnerBuilder::new(jobs.client(), settings(1));
    let agent = Agent {
        lookup: builder.instance_lookup(),
        label: "main",
        inbox: false,
        seen_run: Default::default(),
    };
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(
        args(agent_graph(Behavior::Result("x"), agent), false).with_shutdown(shutdown.clone()),
    ));
    until(|| kinds(&jobs) == vec!["started"]).await;
    tokio::time::sleep(Duration::from_secs(300)).await;
    assert_eq!(kinds(&jobs), vec!["started"]);
    assert_eq!(jobs.triggers_waiting(), 1);
    open.store(true, Ordering::SeqCst);
    until(|| kinds(&jobs).contains(&"completed")).await;
    until(|| jobs.triggers_waiting() == 0).await;
    shutdown.cancel();
    done.await.expect("joined").expect("started");
}

#[tokio::test(start_paused = true)]
async fn given_an_acknowledgement_that_fails_when_the_run_starts_then_the_run_goes_on_and_completes()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.fail_next_delivery(Error::InfraMissing("the consumer".to_owned()));
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| has_terminal(&jobs)).await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack]);
    running.stop().await;
}
