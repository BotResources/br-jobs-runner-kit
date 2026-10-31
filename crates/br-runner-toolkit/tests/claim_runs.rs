mod support;

use br_runner_jobs::test_double::{Call, ScriptedJobs};
use serde_json::json;
use support::*;

fn claim_ids(jobs: &ScriptedJobs) -> Vec<uuid::Uuid> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Claim { claim_id } => Some(claim_id),
            _ => None,
        })
        .collect()
}

fn served(n: u128) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "version": 2,
        "run": serde_json::from_slice::<serde_json::Value>(&trigger(n)).expect("trigger")
    }))
    .expect("bytes")
}

fn refusal(reason: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({ "version": 2, "reason": reason })).expect("bytes")
}

#[tokio::test(start_paused = true)]
async fn given_claim_mode_and_a_served_run_when_the_graph_ends_then_the_run_is_reported_and_no_trigger_acknowledged()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    claim_mode(&jobs);
    jobs.push_claim_reply(Ok(served(1)));
    jobs.push_claim_reply(Ok(refusal("not_in_claim_mode")));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| has_terminal(&jobs)).await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    assert!(jobs.deliveries().is_empty());
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_claim_mode_when_the_runner_has_free_slots_then_it_sends_one_first_claim_per_slot_each_with_a_new_id()
 {
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    let jobs = ScriptedJobs::new("analyst", "i1");
    claim_mode(&jobs);
    jobs.push_claim_reply(Ok(served(1)));
    jobs.push_claim_reply(Ok(served(2)));
    jobs.push_claim_reply(Ok(refusal("not_in_claim_mode")));
    let running = run(
        &jobs,
        |b| b,
        2,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| kinds(&jobs).len() == 2).await;
    let ids = claim_ids(&jobs);
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    gate.notify_waiters();
    gate.notify_one();
    gate.notify_one();
    running.shutdown.cancel();
    gate.notify_waiters();
    let _ = running.done.await;
}

#[tokio::test(start_paused = true)]
async fn given_an_empty_answer_when_it_comes_then_the_next_claim_has_a_new_claim_id() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    claim_mode(&jobs);
    jobs.push_claim_reply(Ok(br"{}".to_vec()));
    jobs.push_claim_reply(Ok(refusal("not_in_claim_mode")));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| claim_ids(&jobs).len() == 2).await;
    let ids = claim_ids(&jobs);
    assert_ne!(ids[0], ids[1]);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_claim_without_answer_when_the_next_beat_comes_then_the_same_claim_id_is_sent_again()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    claim_mode(&jobs);
    jobs.push_claim_reply(Err(br_runner_jobs::Error::InfraMissing(
        "the bus".to_owned(),
    )));
    jobs.push_claim_reply(Ok(refusal("not_in_claim_mode")));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| claim_ids(&jobs).len() == 2).await;
    let ids = claim_ids(&jobs);
    assert_eq!(ids[0], ids[1]);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_busy_refusal_when_it_carries_a_delay_then_the_same_claim_id_is_sent_after_the_delay()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    claim_mode(&jobs);
    jobs.push_claim_reply(Ok(serde_json::to_vec(
        &json!({ "version": 2, "reason": "busy", "retry_after_ms": 2000 }),
    )
    .expect("bytes")));
    jobs.push_claim_reply(Ok(refusal("not_in_claim_mode")));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| claim_ids(&jobs).len() == 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(claim_ids(&jobs).len(), 1);
    until(|| claim_ids(&jobs).len() == 2).await;
    let ids = claim_ids(&jobs);
    assert_eq!(ids[0], ids[1]);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_an_unknown_instance_refusal_when_the_next_beat_comes_then_the_same_claim_id_is_sent_again()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    claim_mode(&jobs);
    jobs.push_claim_reply(Ok(refusal("instance_unknown")));
    jobs.push_claim_reply(Ok(refusal("not_in_claim_mode")));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| claim_ids(&jobs).len() == 2).await;
    let ids = claim_ids(&jobs);
    assert_eq!(ids[0], ids[1]);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_spent_claim_refusal_when_it_comes_then_the_next_claim_has_a_new_claim_id() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    claim_mode(&jobs);
    jobs.push_claim_reply(Ok(refusal("claim_spent")));
    jobs.push_claim_reply(Ok(refusal("not_in_claim_mode")));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| claim_ids(&jobs).len() == 2).await;
    let ids = claim_ids(&jobs);
    assert_ne!(ids[0], ids[1]);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_not_in_claim_mode_refusal_when_it_comes_then_no_claim_is_sent_until_the_mode_feed_gives_a_value()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    claim_mode(&jobs);
    jobs.push_claim_reply(Ok(refusal("not_in_claim_mode")));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| claim_ids(&jobs).len() == 1).await;
    tokio::time::sleep(std::time::Duration::from_secs(120)).await;
    assert_eq!(claim_ids(&jobs).len(), 1);
    jobs.push_claim_reply(Ok(refusal("malformed_claim")));
    jobs.push_mode(Ok(br_runner_jobs::ModeEvent::Mode(
        br_runner_jobs::DispatchMode::Claim,
    )));
    until(|| claim_ids(&jobs).len() == 2).await;
    running.stop().await;
}
