use std::collections::HashSet;
use std::sync::Arc;

use br_runner_jobs::{CancelEvent, DispatchMode, Error, ModeEvent};
use uuid::Uuid;

use crate::shared::{LoopState, Mode, Shared};

fn mode_of(event: ModeEvent) -> Mode {
    match event {
        ModeEvent::Mode(DispatchMode::Push) => Mode::Push,
        ModeEvent::Mode(DispatchMode::Claim) => Mode::Claim,
        _ => Mode::NoEntry,
    }
}

pub(crate) async fn dispatch_mode(shared: Arc<Shared>) {
    loop {
        match shared.calls.client().dispatch_modes().await {
            Err(error) => tracing::error!(%error, "the feed of the dispatch mode cannot be opened"),
            Ok(mut feed) => loop {
                match feed.next().await {
                    Ok(event) => {
                        let mode = mode_of(event);
                        shared.set_claims_stopped(false);
                        shared.loop_state.send_modify(|state| state.mode = mode);
                    }
                    Err(Error::Unreadable(text)) => {
                        tracing::error!(%text, "an event of the dispatch mode cannot be decoded");
                    }
                    Err(error) => {
                        tracing::error!(%error, "the feed of the dispatch mode ended");
                        break;
                    }
                }
            },
        }
        shared.next_beat().await;
    }
}

pub(crate) async fn cancel_orders(shared: Arc<Shared>) {
    loop {
        shared
            .loop_state
            .send_modify(|state: &mut LoopState| state.replay_complete = false);
        match shared.calls.client().cancel_orders().await {
            Err(error) => tracing::error!(%error, "the feed of the cancel orders cannot be opened"),
            Ok(mut feed) => {
                let mut replayed: HashSet<Uuid> = HashSet::new();
                let mut complete = false;
                loop {
                    match feed.next().await {
                        Ok(CancelEvent::OrderMade { run_id }) => {
                            if complete {
                                shared.order_made(run_id);
                            } else {
                                replayed.insert(run_id);
                            }
                        }
                        Ok(CancelEvent::OrderDeleted { run_id }) => {
                            if complete {
                                shared.order_deleted(run_id);
                            } else {
                                replayed.remove(&run_id);
                            }
                        }
                        Ok(CancelEvent::ReplayComplete) => {
                            complete = true;
                            shared.replace_orders(std::mem::take(&mut replayed));
                            shared
                                .loop_state
                                .send_modify(|state| state.replay_complete = true);
                        }
                        Ok(_) => {}
                        Err(Error::Unreadable(text)) => {
                            tracing::error!(%text, "a cancel order cannot be decoded");
                        }
                        Err(error) => {
                            tracing::error!(%error, "the feed of the cancel orders ended");
                            break;
                        }
                    }
                }
            }
        }
        shared
            .loop_state
            .send_modify(|state| state.replay_complete = false);
        shared.next_beat().await;
    }
}
