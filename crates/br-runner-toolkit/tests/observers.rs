mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_jobs::{QueueObserver, QueueReport, Removal};
use serde_json::json;
use support::*;
use tracing::Level;

struct Author {
    reports: Mutex<Vec<QueueReport>>,
    panic_on_first: AtomicBool,
}

impl Author {
    fn new(panic_on_first: bool) -> Arc<Self> {
        Arc::new(Self {
            reports: Mutex::new(Vec::new()),
            panic_on_first: AtomicBool::new(panic_on_first),
        })
    }

    fn reports(&self) -> Vec<QueueReport> {
        self.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl QueueObserver for Author {
    fn report(&self, report: &QueueReport) {
        self.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(report.clone());
        if self.panic_on_first.swap(false, Ordering::SeqCst) {
            panic!("the observer of the author panics");
        }
    }
}

fn refusing_upload(jobs: &ScriptedJobs) {
    jobs.graphql_default(json!({
        "jobsUploadResult": { "success": false, "reason": "too_big", "message": "the answer is too big" }
    }));
    jobs.push_trigger(call_with(trigger(1), &[("mode", json!("SYNC"))]));
}

#[tokio::test(start_paused = true)]
async fn given_an_observer_of_the_author_when_a_write_is_delivered_and_another_removed_then_it_gets_both_reports_and_the_library_logs_each_removal_once()
 {
    let (logs, _guard) = Logs::capture();
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    refusing_upload(&jobs);
    let author = Author::new(false);
    let running = run(
        &jobs,
        |builder| builder.with_queue_observer(author.clone()),
        1,
        args(single(Behavior::Result("x")), false),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let reports = author.reports();
    assert!(
        reports
            .iter()
            .any(|report| matches!(report, QueueReport::Delivered { .. }))
    );
    assert!(reports.iter().any(|report| matches!(
        report,
        QueueReport::Removed {
            removal: Removal::Refused(_),
            ..
        }
    )));
    let removed = reports
        .iter()
        .filter(|report| matches!(report, QueueReport::Removed { .. }))
        .count();
    assert_eq!(removed, 2);
    assert_eq!(
        logs.containing(Level::ERROR, "a write of the run was removed"),
        removed
    );
    assert_eq!(logs.containing(Level::ERROR, &id(201).to_string()), removed);
}

#[tokio::test(start_paused = true)]
async fn given_an_observer_of_the_author_that_panics_when_a_report_comes_then_the_panic_is_logged_and_the_run_ends_normally()
 {
    let (logs, _guard) = Logs::capture();
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let author = Author::new(true);
    let running = run(
        &jobs,
        |builder| builder.with_queue_observer(author.clone()),
        1,
        args(single(Behavior::Result("x")), false),
    );
    until(|| has_terminal(&jobs)).await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    assert_eq!(
        logs.containing(Level::ERROR, "the observer of the runner author panicked"),
        1
    );
    assert!(author.reports().len() >= 2);
    running.stop().await;
}
