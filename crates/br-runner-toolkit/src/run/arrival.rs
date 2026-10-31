use std::sync::Arc;

use br_runner_jobs::{AgentBook, Arrival, Work};
use tokio::task::JoinHandle;

use crate::record::RunRecord;
use crate::shared::Shared;

pub(crate) fn spawn(
    shared: Arc<Shared>,
    record: Arc<RunRecord>,
    work: Arc<Work>,
    book: Arc<AgentBook>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut was_cut = false;
        loop {
            match work.arrival_feed().await {
                Err(error) => {
                    tracing::error!(run_id = %record.run_id, %error, "the arrival feed of the run cannot be opened");
                    book.arrival_ended(&error);
                    was_cut = true;
                }
                Ok(mut feed) => {
                    if was_cut {
                        book.arrival_open_again();
                    }
                    loop {
                        match feed.next().await {
                            Ok(arrival) => {
                                if let Arrival::Unreadable { error, .. } = &arrival {
                                    tracing::error!(run_id = %record.run_id, %error, "an event of the arrival feed cannot be decoded");
                                }
                                book.arrival(&arrival);
                            }
                            Err(error) => {
                                tracing::error!(run_id = %record.run_id, %error, "the arrival feed of the run ended");
                                book.arrival_ended(&error);
                                was_cut = true;
                                break;
                            }
                        }
                    }
                }
            }
            shared.next_beat().await;
        }
    })
}
