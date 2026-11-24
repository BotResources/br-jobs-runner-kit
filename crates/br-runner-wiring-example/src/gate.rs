use std::sync::Arc;

use br_runner_jobs::{MarkGate, mark_gate};
use br_runner_jobs_client::Gate;
use br_runner_threads::{EntryOutcome, ThreadAccessAdapter};

pub fn thread_mark_gate(adapter: Arc<ThreadAccessAdapter>) -> MarkGate {
    mark_gate(move |thread, entry| {
        let adapter = Arc::clone(&adapter);
        async move {
            match adapter.entry_stored(&thread, entry).await {
                EntryOutcome::Stored => Gate::Stored,
                EntryOutcome::Dropped => Gate::Dropped,
            }
        }
    })
}
