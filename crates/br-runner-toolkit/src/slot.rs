use std::sync::Arc;
use std::time::Duration;

use br_graph_contract::Fields;
use br_runner_jobs::{Error, Work};
use tokio::sync::watch;
use uuid::Uuid;

use crate::plan::Plan;
use crate::run::{Source, execute};
use crate::shared::{LoopState, Mode, Shared};

async fn unusable(rx: &mut watch::Receiver<LoopState>, mode: Mode) {
    let _ = rx
        .wait_for(|state| !(state.can_ask() && state.mode == mode))
        .await;
}

async fn next_trigger(shared: &Arc<Shared>) -> Work {
    loop {
        let mut guard = shared.triggers.lock().await;
        if guard.feed.is_none() {
            if guard.closed_at_beat == Some(shared.current_beat()) {
                drop(guard);
                shared.next_beat().await;
                continue;
            }
            match shared.calls.triggers(shared.settings.ack_wait()).await {
                Ok(feed) => {
                    guard.feed = Some(feed);
                    guard.closed_at_beat = None;
                }
                Err(error) => {
                    tracing::error!(%error, "the feed of the triggers cannot be opened");
                    guard.closed_at_beat = Some(shared.current_beat());
                    drop(guard);
                    shared.next_beat().await;
                    continue;
                }
            }
        }
        let Some(feed) = guard.feed.as_mut() else {
            continue;
        };
        match feed.next().await {
            Ok(work) => return work,
            Err(unreadable) if matches!(unreadable.error, Error::Unreadable(_)) => {
                tracing::error!(error = %unreadable.error, "a trigger cannot be decoded");
            }
            Err(unreadable) => {
                tracing::error!(error = %unreadable.error, "the feed of the triggers ended");
                guard.feed = None;
                guard.closed_at_beat = Some(shared.current_beat());
                drop(guard);
                shared.next_beat().await;
            }
        }
    }
}

pub(crate) fn settle_known(shared: &Arc<Shared>, known: &crate::record::RunRecord, work: Work) {
    if !known.started_pending() {
        shared.calls.acknowledge(Arc::new(work));
    }
}

async fn push_step<F: Fields>(
    shared: &Arc<Shared>,
    plan: &Arc<Plan<F>>,
    rx: &mut watch::Receiver<LoopState>,
) {
    let work = tokio::select! {
        work = next_trigger(shared) => work,
        () = unusable(rx, Mode::Push) => return,
        () = shared.drain.cancelled() => return,
    };
    if let Some(work) = given_back_when_draining(shared, work) {
        execute(shared, plan, work, Source::Trigger).await;
    }
}

pub(crate) fn given_back_when_draining(shared: &Arc<Shared>, work: Work) -> Option<Work> {
    if shared.draining() {
        shared.calls.give_back(work);
        return None;
    }
    Some(work)
}

async fn pause(shared: &Arc<Shared>, delay: Option<Duration>) {
    tokio::select! {
        () = async {
            match delay {
                Some(delay) => tokio::time::sleep(delay).await,
                None => shared.next_beat().await,
            }
        } => {}
        () = shared.drain.cancelled() => {}
    }
}

fn dropped_for_drain(shared: &Arc<Shared>, claim_id: Uuid, work: &Work) -> bool {
    if !shared.draining() {
        return false;
    }
    tracing::error!(
        run_id = %work.run_id(),
        job_id = %work.job_id(),
        %claim_id,
        "not started, the drain began: jobs gave this run to a claim in flight"
    );
    true
}

async fn claim_step<F: Fields>(shared: &Arc<Shared>, plan: &Arc<Plan<F>>, held: &mut Option<Uuid>) {
    let claim_id = held.take().unwrap_or_else(Uuid::now_v7);
    let epoch = shared.mode_epoch();
    let reply = shared
        .calls
        .claim(claim_id, shared.settings.claim_wait())
        .await;
    match reply {
        Ok(Some(work)) => {
            if !dropped_for_drain(shared, claim_id, &work) {
                execute(shared, plan, work, Source::Claim).await;
            }
        }
        Ok(None) => {}
        Err(Error::Refused(refusal)) => {
            use contract_jobs::runner::ClaimRefusal as Reason;
            match refusal.reason.as_str() {
                reason if reason == Reason::Busy.as_str() => {
                    tracing::warn!(%claim_id, "the claim is repeated after a busy refusal");
                    *held = Some(claim_id);
                    pause(shared, refusal.retry_after_ms.map(Duration::from_millis)).await;
                }
                reason if reason == Reason::InstanceUnknown.as_str() => {
                    tracing::warn!(%claim_id, "the claim is repeated after an unknown-instance refusal");
                    *held = Some(claim_id);
                    pause(shared, None).await;
                }
                reason if reason == Reason::ClaimSpent.as_str() => {}
                reason => {
                    if reason == Reason::MalformedClaim.as_str() {
                        tracing::error!(%claim_id, "jobs refused the claim as malformed");
                    }
                    shared.stop_claims(epoch);
                }
            }
        }
        Err(error @ Error::InvalidInput(_)) => {
            tracing::error!(%claim_id, %error, "the claim could not be made, there are no more claims");
            shared.stop_claims(epoch);
        }
        Err(error) => {
            tracing::warn!(%claim_id, %error, "the claim is repeated after a missing answer");
            *held = Some(claim_id);
            pause(shared, None).await;
        }
    }
}

fn must_wait(state: LoopState, holds_claim: bool, claims_blocked: bool) -> bool {
    !state.can_ask() || (claims_blocked && (state.mode != Mode::Push || holds_claim))
}

pub(crate) async fn run<F: Fields>(shared: Arc<Shared>, plan: Arc<Plan<F>>) {
    let mut rx = shared.loop_state.subscribe();
    let mut held: Option<Uuid> = None;
    loop {
        if shared.draining() {
            return;
        }
        let state = *rx.borrow_and_update();
        let claiming = held.is_some() || state.mode == Mode::Claim;
        let claims_blocked = claiming && shared.claims_stopped();
        if must_wait(state, held.is_some(), claims_blocked) {
            tokio::select! {
                _ = rx.changed() => {}
                () = shared.drain.cancelled() => return,
            }
            continue;
        }
        if claiming && !claims_blocked {
            claim_step(&shared, &plan, &mut held).await;
        } else {
            push_step(&shared, &plan, &mut rx).await;
        }
        if shared.host_has_taken_its_run() {
            shared.drain.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use br_runner_jobs::test_double::{Delivery, ScriptedJobs};

    use tracing::Level;

    use super::{dropped_for_drain, given_back_when_draining, must_wait};
    use crate::instances::Instances;
    use crate::shared::{LoopState, Mode};
    use crate::unit_support::logs::Logs;
    use crate::unit_support::{delivered_work, id, shared};

    #[tokio::test]
    async fn given_a_drain_when_a_claim_returns_a_run_then_it_is_not_started_and_one_error_names_the_run()
     {
        let (logs, _guard) = Logs::capture();
        let jobs = ScriptedJobs::new("analyst", "i1");
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
        let jobs = ScriptedJobs::new("analyst", "i1");
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
        let jobs = ScriptedJobs::new("analyst", "i1");
        let shared = shared(&jobs, None, &Arc::new(Instances::default()));
        shared.drain.cancel();
        assert!(given_back_when_draining(&shared, delivered_work(&jobs, 1).await).is_none());
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(jobs.deliveries(), vec![Delivery::GiveBack]);
    }

    #[tokio::test]
    async fn given_no_drain_when_a_trigger_arrives_then_it_is_kept() {
        let jobs = ScriptedJobs::new("analyst", "i1");
        let shared = shared(&jobs, None, &Arc::new(Instances::default()));
        assert!(given_back_when_draining(&shared, delivered_work(&jobs, 1).await).is_some());
        assert!(jobs.deliveries().is_empty());
    }
}
