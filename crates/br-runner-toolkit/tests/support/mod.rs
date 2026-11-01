#![allow(dead_code)] // each test file uses a part of the support

mod boom;
mod logs;
mod nodes;

use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_graph_contract::Graph;
use br_runner_jobs::test_double::{Call, Fact, ScriptedJobs, trigger_json};
use br_runner_jobs::{CancelEvent, DispatchMode, ModeEvent};
use br_runner_toolkit::{
    DrainReport, Error, RunRefusal, RunnerBuilder, Settings, StartArgs, StartedRun, run_start,
};
use serde_json::json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[allow(unused_imports)] // each test file uses a part of the support
pub use boom::*;
#[allow(unused_imports)] // each test file uses a part of the support
pub use logs::*;
pub use nodes::*;

pub fn args(graph: Arc<Graph<Flow>>, can_restart: bool) -> StartArgs<Flow> {
    StartArgs::new(
        graph,
        run_start(|_input| async {
            Ok::<_, RunRefusal>(StartedRun::new(Flow::default(), vec!["work".to_owned()]))
        }),
        can_restart,
    )
}

pub fn settings(slots: u32) -> Settings {
    Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(slots)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(60))
        .cancel_grace(Duration::from_secs(1))
        .claim_wait(Duration::from_secs(5))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
        .build()
        .expect("settings")
}

pub fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

pub fn passport() -> Passport {
    Passport::human(
        id(900),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::from_value(json!({ "email": "ocelot@example.test" })).expect("claims"),
    )
}

pub fn trigger(n: u128) -> Vec<u8> {
    serde_json::to_vec(
        &trigger_json(id(100 + n), id(200 + n), id(300), id(400), &passport()).expect("trigger"),
    )
    .expect("bytes")
}

pub fn push_mode(jobs: &ScriptedJobs) {
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Push)));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
}

pub fn claim_mode(jobs: &ScriptedJobs) {
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Claim)));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
}

pub struct Running {
    pub jobs: Arc<ScriptedJobs>,
    pub shutdown: CancellationToken,
    pub done: JoinHandle<Result<DrainReport, Error>>,
}

pub fn run(
    jobs: &Arc<ScriptedJobs>,
    builder: impl FnOnce(RunnerBuilder) -> RunnerBuilder,
    slots: u32,
    args: StartArgs<Flow>,
) -> Running {
    let shutdown = CancellationToken::new();
    let runner = builder(RunnerBuilder::new(jobs.client(), settings(slots)));
    let done = tokio::spawn(runner.start(args.with_shutdown(shutdown.clone())));
    Running {
        jobs: Arc::clone(jobs),
        shutdown,
        done,
    }
}

impl Running {
    pub async fn stop(self) -> DrainReport {
        self.shutdown.cancel();
        self.done.await.expect("joined").expect("started")
    }
}

pub async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(600), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the condition came true");
}

pub fn facts(jobs: &ScriptedJobs) -> Vec<Fact> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Fact(fact) => Some(fact),
            _ => None,
        })
        .collect()
}

pub fn has_terminal(jobs: &ScriptedJobs) -> bool {
    facts(jobs)
        .iter()
        .any(|fact| matches!(fact, Fact::Completed { .. } | Fact::Failed { .. }))
}

pub fn kinds(jobs: &ScriptedJobs) -> Vec<&'static str> {
    facts(jobs)
        .iter()
        .map(|fact| match fact {
            Fact::Started { .. } => "started",
            Fact::Plan { .. } => "plan",
            Fact::Step { .. } => "step",
            Fact::Completed { .. } => "completed",
            Fact::Failed { .. } => "failed",
        })
        .collect()
}

pub fn call_with(trigger: Vec<u8>, fields: &[(&str, serde_json::Value)]) -> Vec<u8> {
    let mut value: serde_json::Value = serde_json::from_slice(&trigger).expect("json");
    let object = value.as_object_mut().expect("object");
    for (key, field) in fields {
        object.insert((*key).to_owned(), field.clone());
    }
    serde_json::to_vec(&value).expect("bytes")
}

pub fn saved_state_reply(state: serde_json::Value) -> serde_json::Value {
    json!({ "jobsSavedState": {
        "success": true, "state": state, "agentIds": [], "mailAddresses": []
    } })
}

pub fn writes_ok() -> serde_json::Value {
    json!({
        "jobsUploadResult": { "success": true },
        "jobsSaveState": { "success": true }
    })
}

pub fn failed_fact(jobs: &ScriptedJobs) -> Option<br_runner_jobs::Failed> {
    facts(jobs).into_iter().find_map(|fact| match fact {
        Fact::Failed { failed, .. } => Some(failed),
        _ => None,
    })
}

pub fn claim_ids(jobs: &ScriptedJobs) -> Vec<Uuid> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Claim { claim_id } => Some(claim_id),
            _ => None,
        })
        .collect()
}

pub fn served(n: u128) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "version": 2,
        "run": serde_json::from_slice::<serde_json::Value>(&trigger(n)).expect("trigger")
    }))
    .expect("bytes")
}

pub fn refusal(reason: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({ "version": 2, "reason": reason })).expect("bytes")
}

pub async fn refusal_error(reason: &str) -> br_runner_jobs::Error {
    let jobs = ScriptedJobs::new("analyst", "refusals");
    jobs.push_claim_reply(Ok(refusal(reason)));
    jobs.client()
        .claim(Uuid::now_v7(), Duration::from_secs(1))
        .await
        .expect_err("a refusal")
}

pub fn refuse_the_first_started_fact(jobs: &ScriptedJobs, error: br_runner_jobs::Error) {
    let mut refused = false;
    jobs.answer_facts(move || {
        if refused {
            Ok(())
        } else {
            refused = true;
            Err(error.clone())
        }
    });
}

pub async fn quiet_for(duration: Duration) {
    tokio::time::sleep(duration).await;
}
