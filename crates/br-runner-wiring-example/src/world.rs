use std::sync::{Arc, OnceLock};
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_llm_messages::AgentId;
use br_runner_jobs_client::test_double::{ScriptedJobs, trigger_json};
use br_runner_jobs_client::{CancelEvent, DispatchMode, ModeEvent, RunnerType};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_toolkit::{
    HostRun, RunRefusal, Settings, StartArgs, StartedRun, TestHost, run_start,
};
use contract_jobs::graphql_roots::UNREAD_ROOT;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::graph::mail_graph;
use crate::node::{MailNode, Observed};
use crate::wiring::Variant;

type Failure = Box<dyn std::error::Error + Send + Sync>;

pub fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

pub fn unread_item(agent: AgentId, subject: &str) -> Value {
    json!({ UNREAD_ROOT: {
        "success": true, "reason": null, "message": null,
        "items": [{ "itemId": id(101), "seq": 1, "itemKind": "CC_NOTICE", "address": agent.as_uuid(),
            "mailId": id(201), "subject": subject, "attachments": null, "noticeType": null }],
    } })
}

#[derive(Clone)]
pub struct World {
    pub jobs: Arc<ScriptedJobs>,
    pub threads: MemoryThreads,
    pub observed: Arc<Observed>,
    pub variant: Variant,
    pub patience: Duration,
}

impl World {
    pub fn new(variant: Variant, patience: Duration) -> Result<Self, Failure> {
        let jobs = ScriptedJobs::new(RunnerType::new("analyst")?, "instance-1");
        jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Push)));
        jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
        let passport = Passport::human(
            id(5),
            false,
            true,
            AuthMethod::Jwt,
            None,
            PassportClaims::from_value(json!({}))
                .map_err(|claims| format!("invalid claims: {claims}"))?,
        );
        let trigger = trigger_json(id(2), id(1), id(3), id(4), &passport)?;
        jobs.push_trigger(serde_json::to_vec(&trigger)?);
        Ok(Self {
            jobs,
            threads: MemoryThreads::new(),
            observed: Arc::new(Observed::default()),
            variant,
            patience,
        })
    }
}

fn settings() -> br_runner_toolkit::SettingsBuilder {
    Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(1)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(1))
        .cancel_grace(Duration::from_secs(1))
        .claim_wait(Duration::from_secs(5))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
}

pub async fn run_in_host(world: &World) -> Result<HostRun, Failure> {
    let lookup = Arc::new(OnceLock::new());
    let jobs = Arc::clone(&world.jobs);
    let node = Arc::new(MailNode {
        lookup: Arc::clone(&lookup),
        threads: world.threads.client(),
        variant: world.variant,
        seeder: Arc::new(move |agent| jobs.graphql_default(unread_item(agent, "quarterly report"))),
        patience: world.patience,
        observed: Arc::clone(&world.observed),
    });
    let starter = run_start(|input| async move {
        Ok::<_, RunRefusal>(StartedRun::new(
            input.restored.unwrap_or_default(),
            vec!["analyst".to_owned()],
        ))
    });
    let args = StartArgs::new(mail_graph(node)?, starter, false);
    let host = TestHost::new(world.jobs.client(), settings(), args)?;
    lookup
        .set(host.instance_lookup())
        .map_err(|_| "the lookup was set twice")?;
    Ok(host.run().await?)
}
