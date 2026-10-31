mod support;

use br_runner_jobs::CancelEvent;
use br_runner_jobs::test_double::{Delivery, ScriptedJobs};
use support::*;

#[tokio::test(start_paused = true)]
async fn given_a_cancel_order_for_a_run_in_flight_when_it_comes_then_the_run_ends_with_no_terminal_fact_and_the_slot_is_free() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::UntilCancelled), false));
    until(|| kinds(&jobs) == vec!["started"]).await;
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(101) }));
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert_eq!(kinds(&jobs), vec!["started"]);
    jobs.push_trigger(trigger(2));
    until(|| kinds(&jobs).len() == 2).await;
    running.stop().await;
    assert!(!has_terminal(&jobs));
}

#[tokio::test(start_paused = true)]
async fn given_a_cancel_order_known_when_the_run_is_taken_then_the_graph_is_not_started_and_no_terminal_fact_is_sent() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    jobs.push_mode(Ok(br_runner_jobs::ModeEvent::Mode(br_runner_jobs::DispatchMode::Push)));
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(101) }));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
    jobs.push_trigger(trigger(1));
    let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&started);
    let marking = br_runner_toolkit::StartArgs::new(
        single(Behavior::Result("x")),
        br_runner_toolkit::run_start(move |_input| {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            async {
                Ok::<_, br_runner_toolkit::RunRefusal>(br_runner_toolkit::StartedRun::new(
                    Flow::default(),
                    vec!["work".to_owned()],
                ))
            }
        }),
        false,
    );
    let running = run(&jobs, |b| b, 1, marking);
    until(|| jobs.deliveries() == vec![Delivery::Ack]).await;
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert_eq!(kinds(&jobs), vec!["started"]);
    assert!(!started.load(std::sync::atomic::Ordering::SeqCst));
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_cancel_order_for_another_run_when_it_comes_then_the_run_goes_on() {
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(999) }));
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    gate.notify_one();
    until(|| has_terminal(&jobs)).await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    running.stop().await;
}
