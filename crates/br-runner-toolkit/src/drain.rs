use std::collections::{HashSet, VecDeque};
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
    pub executor_running: bool,
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
    let instances_without_return = if aborted {
        record.running_hooks()
    } else {
        Vec::new()
    };
    DrainedRun {
        job_id: record.job_id,
        run_id: record.run_id,
        aborted,
        executor_running: aborted && record.executor_running(),
        instances_without_return,
        unsent,
    }
}

fn abort_run(shared: &Arc<Shared>, record: &Arc<RunRecord>) -> DrainedRun {
    record.set_phase(Phase::Aborted);
    record.cancel.trigger();
    hooks::call_all(shared, record);
    let run = drained_run(record, true);
    tracing::error!(
        run_id = %record.run_id,
        job_id = %record.job_id,
        executor_running = run.executor_running,
        instances_without_return = ?run.instances_without_return,
        "the drain aborts a run that is still in flight"
    );
    record.stop_arrival();
    drop(record.take_book());
    shared.instances.remove_run(record, Phase::Aborted);
    record.take_queue();
    shared.forget_run(record.run_id);
    run
}

async fn wait_for_runs(shared: &Arc<Shared>, slots: &mut VecDeque<JoinHandle<()>>) {
    while let Some(slot) = slots.front_mut() {
        let _ = slot.await;
        slots.pop_front();
    }
    shared.closings.all_ended().await;
}

pub(crate) async fn drain(shared: &Arc<Shared>, slots: Vec<JoinHandle<()>>) -> DrainReport {
    shared.drain.cancel();
    shared.status.send_replace(PresenceStatus::Draining);
    let mut slots: VecDeque<JoinHandle<()>> = slots.into();
    let waited = tokio::time::timeout(
        shared.settings.drain_timeout(),
        wait_for_runs(shared, &mut slots),
    )
    .await;
    let mut report = DrainReport::default();
    if waited.is_err() {
        for slot in &slots {
            slot.abort();
        }
        for slot in slots {
            let _ = slot.await;
        }
        let closings = shared.closings.take_all();
        for (handle, _) in &closings {
            handle.abort();
        }
        let mut closing_runs = Vec::new();
        for (handle, record) in closings {
            let _ = handle.await;
            closing_runs.push(record);
        }
        let closing_ids: HashSet<Uuid> = closing_runs.iter().map(|record| record.run_id).collect();
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
    }
    shared.triggers.lock().await.feed = None;
    report
}
