mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use br_runner_jobs::Error;
use br_runner_jobs::test_double::{Delivery, ScriptedJobs};
use br_runner_toolkit::{RunRefusal, StartArgs, StartedRun, run_start};
use support::*;

fn marking(executed: &Arc<AtomicBool>) -> StartArgs<Flow> {
    let flag = Arc::clone(executed);
    StartArgs::new(
        single(Behavior::Result("x")),
        run_start(move |_input| {
            flag.store(true, Ordering::SeqCst);
            async { Ok::<_, RunRefusal>(StartedRun::new(Flow::default(), vec!["work".to_owned()])) }
        }),
        false,
    )
}

#[tokio::test(start_paused = true)]
async fn given_a_started_fact_that_jobs_refuses_when_the_trigger_comes_then_it_is_acknowledged_and_dropped_the_run_is_not_executed_and_the_slot_serves_the_next_trigger()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    refuse_the_first_started_fact(&jobs, refusal_error("claim_spent"));
    let executed = Arc::new(AtomicBool::new(false));
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, marking(&executed));
    until(|| jobs.deliveries() == vec![Delivery::Ack]).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(!executed.load(Ordering::SeqCst));
    assert_eq!(kinds(&jobs), vec!["started"]);
    jobs.push_trigger(trigger(2));
    until(|| has_terminal(&jobs)).await;
    assert_eq!(kinds(&jobs), vec!["started", "started", "completed"]);
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack, Delivery::Ack]);
    assert!(executed.load(Ordering::SeqCst));
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_started_fact_that_fails_for_another_final_error_when_the_trigger_comes_then_it_is_not_acknowledged_and_the_run_is_not_executed()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    let mut failed = false;
    jobs.answer_started(move || {
        if failed {
            Ok(())
        } else {
            failed = true;
            Err(Error::InfraMissing("the stream".to_owned()))
        }
    });
    let executed = Arc::new(AtomicBool::new(false));
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, marking(&executed));
    until(|| kinds(&jobs) == vec!["started"]).await;
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(jobs.deliveries().is_empty());
    assert!(!executed.load(Ordering::SeqCst));
    jobs.push_trigger(trigger(2));
    until(|| has_terminal(&jobs)).await;
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack]);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_started_fact_that_jobs_refuses_in_claim_mode_when_the_run_is_served_then_it_is_not_executed_and_the_slot_claims_again_with_a_new_claim_id()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    claim_mode(&jobs);
    refuse_the_first_started_fact(&jobs, refusal_error("claim_spent"));
    jobs.push_claim_reply(Ok(served(1)));
    jobs.push_claim_reply(Ok(refusal("not_claim_mode")));
    let executed = Arc::new(AtomicBool::new(false));
    let running = run(&jobs, |b| b, 1, marking(&executed));
    until(|| claim_ids(&jobs).len() == 2).await;
    let ids = claim_ids(&jobs);
    assert_ne!(ids[0], ids[1]);
    assert!(!executed.load(Ordering::SeqCst));
    assert!(jobs.deliveries().is_empty());
    assert_eq!(kinds(&jobs), vec!["started"]);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_started_fact_that_jobs_does_not_answer_when_the_drain_time_ends_then_the_trigger_is_neither_acknowledged_nor_given_back_and_the_report_names_the_aborted_run()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.answer_started(|| Err(Error::Unreadable("the bus is down".to_owned())));
    let executed = Arc::new(AtomicBool::new(false));
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, marking(&executed));
    until(|| kinds(&jobs) == vec!["started"]).await;
    running.shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(61)).await;
    let report = running.done.await.expect("joined").expect("started");
    assert_eq!(report.runs.len(), 1);
    assert!(report.runs[0].aborted);
    assert_eq!(report.runs[0].run_id, id(101));
    assert!(
        report.runs[0]
            .unsent
            .iter()
            .any(|write| write.kind == br_runner_jobs::WriteKind::Started)
    );
    assert!(jobs.deliveries().is_empty());
    assert!(!executed.load(Ordering::SeqCst));
}
