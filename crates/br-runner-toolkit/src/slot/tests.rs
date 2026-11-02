use std::sync::Arc;

use br_runner_jobs::test_double::{Delivery, ScriptedJobs};

use tracing::Level;

use super::{claim_step, dropped_for_drain, given_back_when_draining, must_wait, push_step};
use crate::instances::Instances;
use crate::shared::{LoopState, Mode};
use crate::unit_support::logs::Logs;
use crate::unit_support::{
    delivered_work, id, idle_plan, push_trigger, runner_type, served, shared, started_facts,
};

#[tokio::test]
async fn given_a_drain_when_a_claim_returns_a_run_then_it_is_not_started_and_one_error_names_the_run()
 {
    let (logs, _guard) = Logs::capture();
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    let shared = shared(&jobs, None, &Arc::new(Instances::default()));
    let work = delivered_work(&jobs, 1).await;
    shared.drain.cancel();
    assert!(dropped_for_drain(&shared, id(9), &work));
    assert_eq!(logs.at(Level::ERROR).len(), 1);
    assert_eq!(logs.containing(Level::ERROR, &work.run_id().to_string()), 1);
    assert_eq!(logs.containing(Level::ERROR, "the drain began"), 1);
}

#[tokio::test]
async fn given_no_drain_when_a_claim_returns_a_run_then_it_is_started_and_nothing_is_logged() {
    let (logs, _guard) = Logs::capture();
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    let shared = shared(&jobs, None, &Arc::new(Instances::default()));
    let work = delivered_work(&jobs, 1).await;
    assert!(!dropped_for_drain(&shared, id(9), &work));
    assert!(logs.lines().is_empty());
}

fn state(mode: Mode, replay_complete: bool) -> LoopState {
    LoopState {
        mode,
        replay_complete,
    }
}

#[test]
fn given_a_slot_that_holds_a_claim_id_and_stopped_claims_when_the_mode_is_push_then_it_waits() {
    assert!(must_wait(state(Mode::Push, true), true, true));
}

#[test]
fn given_a_slot_that_holds_nothing_and_stopped_claims_when_the_mode_is_push_then_it_asks_for_a_trigger()
 {
    assert!(!must_wait(state(Mode::Push, true), false, false));
}

#[test]
fn given_a_slot_that_holds_a_claim_id_when_claims_are_open_in_push_mode_then_it_repeats_the_claim()
{
    assert!(!must_wait(state(Mode::Push, true), true, false));
}

#[test]
fn given_stopped_claims_in_claim_mode_then_the_slot_waits_and_a_closed_gate_always_waits() {
    assert!(must_wait(state(Mode::Claim, true), false, true));
    assert!(must_wait(state(Mode::NoEntry, true), false, false));
    assert!(must_wait(state(Mode::Push, false), false, false));
}

#[tokio::test]
async fn given_a_drain_when_a_trigger_arrives_then_it_is_given_back_and_not_executed() {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    let shared = shared(&jobs, None, &Arc::new(Instances::default()));
    shared.drain.cancel();
    assert!(given_back_when_draining(&shared, delivered_work(&jobs, 1).await).is_none());
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(jobs.deliveries(), vec![Delivery::GiveBack]);
}

#[tokio::test]
async fn given_no_drain_when_a_trigger_arrives_then_it_is_kept() {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    let shared = shared(&jobs, None, &Arc::new(Instances::default()));
    assert!(given_back_when_draining(&shared, delivered_work(&jobs, 1).await).is_some());
    assert!(jobs.deliveries().is_empty());
}

async fn round_with_a_trigger_ready_when_the_drain_begins() -> Arc<ScriptedJobs> {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    let shared = shared(&jobs, None, &Arc::new(Instances::default()));
    shared.mode_changed(Mode::Push);
    shared
        .loop_state
        .send_modify(|state| state.replay_complete = true);
    let mut rx = shared.loop_state.subscribe();
    push_trigger(&jobs, 1);
    shared.drain.cancel();
    push_step(&shared, &idle_plan(), &mut rx).await;
    assert_eq!(started_facts(&jobs), 0);
    assert!(shared.runs().is_empty());
    jobs
}

#[tokio::test]
async fn given_a_slot_that_waits_for_a_trigger_when_a_trigger_is_ready_after_the_drain_began_then_the_slot_gives_it_back_and_starts_no_run_in_every_round()
 {
    let mut rounds = Vec::new();
    for _ in 0..96 {
        rounds.push(round_with_a_trigger_ready_when_the_drain_begins().await);
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    for jobs in rounds {
        assert_eq!(jobs.deliveries(), vec![Delivery::GiveBack]);
    }
}

#[tokio::test]
async fn given_a_slot_whose_claim_returns_a_run_after_the_drain_began_when_the_claim_step_ends_then_no_run_is_started_and_the_claim_id_is_not_held()
 {
    let (logs, _guard) = Logs::capture();
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    let shared = shared(&jobs, None, &Arc::new(Instances::default()));
    jobs.push_claim_reply(Ok(served(1)));
    shared.drain.cancel();
    let mut held = None;
    claim_step(&shared, &idle_plan(), &mut held).await;
    assert_eq!(started_facts(&jobs), 0);
    assert!(shared.runs().is_empty());
    assert!(held.is_none());
    assert_eq!(logs.containing(Level::ERROR, "the drain began"), 1);
}
