use std::sync::Arc;

use br_graph_contract::InstanceId;
use br_runner_jobs::{PresenceStatus, WriteName};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::hooks;
use crate::record::{Phase, RunRecord};
use crate::shared::Shared;

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DrainedRun {
    pub job_id: Uuid,
    pub run_id: Uuid,
    pub aborted: bool,
    pub instances_without_return: Vec<InstanceId>,
    pub unsent: Vec<WriteName>,
}

#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct DrainReport {
    pub runs: Vec<DrainedRun>,
}

fn drained_run(record: &Arc<RunRecord>, aborted: bool) -> DrainedRun {
    let unsent = record
        .queue()
        .map(|queue| queue.state().waiting)
        .unwrap_or_default();
    let instances_without_return: Vec<InstanceId> = if aborted {
        record
            .unhooked_instances()
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    } else {
        Vec::new()
    };
    DrainedRun {
        job_id: record.job_id,
        run_id: record.run_id,
        aborted,
        instances_without_return,
        unsent,
    }
}

fn abort_run(shared: &Arc<Shared>, record: &Arc<RunRecord>) -> DrainedRun {
    let run = drained_run(record, true);
    tracing::error!(
        run_id = %record.run_id,
        job_id = %record.job_id,
        executor_running = record.executor_running(),
        instances = run.instances_without_return.len(),
        writes_left = run.unsent.len(),
        "the drain aborts a run that is still in flight"
    );
    record.set_phase(Phase::Aborted);
    hooks::call_all(shared, record);
    record.stop_arrival();
    drop(record.take_book());
    shared.instances.remove_run(record.run_id);
    record.take_queue();
    shared.forget_run(record.run_id);
    run
}

pub(crate) async fn drain(shared: &Arc<Shared>, slots: Vec<JoinHandle<()>>) -> DrainReport {
    shared.drain.cancel();
    shared.status.send_replace(PresenceStatus::Draining);
    let mut slots = slots;
    let waited = tokio::time::timeout(shared.settings.drain_timeout(), async {
        for slot in &mut slots {
            let _ = slot.await;
        }
        shared.closings.all_ended().await;
    })
    .await;
    let mut report = DrainReport::default();
    if waited.is_err() {
        let closing_runs: Vec<Arc<RunRecord>> = shared
            .closings
            .drain_all()
            .into_iter()
            .map(|(handle, record)| {
                handle.abort();
                record
            })
            .collect();
        let closing_ids: Vec<Uuid> = closing_runs.iter().map(|record| record.run_id).collect();
        for slot in &slots {
            slot.abort();
        }
        for record in shared.runs() {
            if !closing_ids.contains(&record.run_id) {
                report.runs.push(abort_run(shared, &record));
            }
        }
        for record in closing_runs {
            let run = drained_run(&record, false);
            record.take_queue();
            shared.forget_run(record.run_id);
            if !run.unsent.is_empty() {
                report.runs.push(run);
            }
        }
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    }
    *shared.triggers.lock().await = None;
    if let Err(error) = shared.calls.delete_presence().await {
        tracing::error!(%error, "the presence could not be deleted, the time to live of the bucket ends it");
    }
    report
}
