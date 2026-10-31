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
        if guard.is_none() {
            match shared.calls.triggers(shared.settings.ack_wait()).await {
                Ok(feed) => *guard = Some(feed),
                Err(error) => {
                    tracing::error!(%error, "the feed of the triggers cannot be opened");
                    drop(guard);
                    shared.next_beat().await;
                    continue;
                }
            }
        }
        let Some(feed) = guard.as_mut() else {
            continue;
        };
        match feed.next().await {
            Ok(work) => return work,
            Err(unreadable) if matches!(unreadable.error, Error::Unreadable(_)) => {
                tracing::error!(error = %unreadable.error, "a trigger cannot be decoded");
            }
            Err(unreadable) => {
                tracing::error!(error = %unreadable.error, "the feed of the triggers ended");
                *guard = None;
                drop(guard);
                shared.next_beat().await;
            }
        }
    }
}

fn settle_known(shared: &Arc<Shared>, work: Work) {
    let acknowledge = shared
        .run(work.run_id())
        .is_some_and(|known| !known.started_pending());
    if acknowledge {
        let shared = Arc::clone(shared);
        tokio::spawn(async move {
            if let Err(error) = shared.calls.acknowledge(&work).await {
                tracing::error!(%error, "the trigger could not be acknowledged");
            }
        });
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
    if shared.draining() {
        if let Err(error) = shared.calls.give_back(work).await {
            tracing::error!(%error, "the trigger could not be given back");
        }
        return;
    }
    if shared.run(work.run_id()).is_some() {
        settle_known(shared, work);
        return;
    }
    execute(shared, plan, work, Source::Trigger).await;
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

async fn claim_step<F: Fields>(shared: &Arc<Shared>, plan: &Arc<Plan<F>>, held: &mut Option<Uuid>) {
    let claim_id = held.take().unwrap_or_else(Uuid::now_v7);
    let reply = shared
        .calls
        .claim(claim_id, shared.settings.claim_wait())
        .await;
    match reply {
        Ok(Some(work)) => {
            if !shared.draining() {
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
                    shared.set_claims_stopped(true);
                }
            }
        }
        Err(error) => {
            tracing::warn!(%claim_id, %error, "the claim is repeated after a missing answer");
            *held = Some(claim_id);
            pause(shared, None).await;
        }
    }
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
        if claiming && shared.claims_stopped() && state.mode == Mode::Push {
            held = None;
            continue;
        }
        if !state.can_ask() || (claiming && shared.claims_stopped()) {
            tokio::select! {
                _ = rx.changed() => {}
                () = shared.drain.cancelled() => return,
            }
            continue;
        }
        if claiming {
            claim_step(&shared, &plan, &mut held).await;
        } else {
            push_step(&shared, &plan, &mut rx).await;
        }
        if shared.single_run() && shared.runs_taken() > 0 {
            shared.drain.cancel();
        }
    }
}
