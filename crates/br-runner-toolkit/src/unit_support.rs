use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_graph_contract::{CancelToken, InstanceId};
use br_runner_jobs::test_double::{ScriptedJobs, trigger_json, work_from_trigger};
use br_runner_jobs::{AgentBook, DeliveryQueue, IgnoreReports, QueueSettings, RunnerType, Work};
use uuid::Uuid;

#[path = "../tests/support/logs.rs"]
pub(crate) mod logs;

use crate::calls::Calls;
use crate::hook::LifecycleHook;
use crate::instances::Instances;
use crate::record::RunRecord;
use crate::settings::Settings;
use crate::shared::Shared;

pub(crate) fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

pub(crate) fn runner_type(text: &str) -> RunnerType {
    RunnerType::new(text).expect("a runner type")
}

pub(crate) fn instance(n: u128) -> InstanceId {
    InstanceId::new(id(n)).expect("a UUIDv7")
}

pub(crate) fn settings() -> Settings {
    Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(1)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(60))
        .cancel_grace(Duration::from_secs(1))
        .claim_wait(Duration::from_secs(5))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
        .build()
        .expect("settings")
}

pub(crate) fn shared(
    jobs: &Arc<ScriptedJobs>,
    hook: Option<LifecycleHook>,
    instances: &Arc<Instances>,
) -> Arc<Shared> {
    Arc::new(Shared::new(
        Calls::new(jobs.client(), None),
        settings(),
        hook,
        None,
        Arc::clone(instances),
    ))
}

pub(crate) fn work(jobs: &Arc<ScriptedJobs>, n: u128) -> Work {
    work_from_trigger(jobs, trigger_of(n)).expect("a work")
}

pub(crate) fn push_trigger(jobs: &Arc<ScriptedJobs>, n: u128) {
    jobs.push_trigger(serde_json::to_vec(&trigger_of(n)).expect("bytes"));
}

pub(crate) fn served(n: u128) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "version": 2, "run": trigger_of(n) })).expect("bytes")
}

pub(crate) async fn delivered_work(jobs: &Arc<ScriptedJobs>, n: u128) -> Work {
    let mut feed = jobs
        .client()
        .triggers(Duration::from_secs(30))
        .await
        .expect("a trigger feed");
    jobs.push_trigger(serde_json::to_vec(&trigger_of(n)).expect("bytes"));
    feed.next()
        .await
        .map_err(|unreadable| unreadable.error.to_string())
        .expect("a work")
}

fn trigger_of(n: u128) -> serde_json::Value {
    let passport = Passport::human(
        id(900),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::from_value(serde_json::json!({ "email": "ocelot@example.test" }))
            .expect("claims"),
    );
    trigger_json(id(100 + n), id(200 + n), id(300), id(400), &passport).expect("trigger")
}

pub(crate) async fn queue_of(work: &Work) -> DeliveryQueue {
    let settings = QueueSettings {
        start_id: Uuid::now_v7(),
        outcome_wait: Duration::from_secs(10),
        retry_wait: Duration::from_secs(1),
        observer: Arc::new(IgnoreReports),
    };
    work.delivery_queue(settings).await.expect("a queue").0
}

pub(crate) fn record(work: &Work) -> Arc<RunRecord> {
    RunRecord::new(
        work.run_id(),
        work.job_id(),
        CancelToken::new().handle().clone(),
    )
}

pub(crate) fn book_with_agent(work: &Work, queue: &DeliveryQueue, node: &str) -> AgentBook {
    let agent = br_runner_jobs::RestoredAgent {
        key: br_runner_jobs::AgentKey::new(node, "main"),
        agent_id: id(500),
        has_inbox: false,
    };
    work.agent_book(queue, vec![agent], false).expect("a book")
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Steps {
    pub steps: Vec<String>,
}

impl br_graph_contract::Fields for Steps {
    type Update = ();

    fn empty_update() {}

    fn validate(&self) -> Result<(), br_graph_contract::Reason> {
        Ok(())
    }

    fn apply(&mut self, _update: ()) -> Result<(), br_graph_contract::Reason> {
        Ok(())
    }
}

pub(crate) fn book_with_agents(
    work: &Work,
    queue: &DeliveryQueue,
    agents: Vec<br_runner_jobs::RestoredAgent>,
) -> AgentBook {
    work.agent_book(queue, agents, false).expect("a book")
}

struct Idle;

impl br_graph_contract::Node<Steps> for Idle {
    fn name(&self) -> &str {
        "work"
    }

    fn kind(&self) -> &str {
        "programmatic"
    }

    fn subkind(&self) -> &str {
        "worker"
    }

    fn create(
        &self,
        _cancel: br_graph_contract::CancelView,
        id: InstanceId,
    ) -> br_graph_contract::CreateFuture<'_, Steps> {
        Box::pin(async move {
            let (instance, _reader, _sender) = br_graph_contract::Instance::<Steps>::channel(id);
            Ok(instance)
        })
    }
}

struct ToEnd;

impl br_graph_contract::Edge<Steps> for ToEnd {
    fn name(&self) -> &str {
        "work"
    }

    fn source(&self) -> &str {
        "work"
    }

    fn targets(&self) -> &[br_graph_contract::Target] {
        &[br_graph_contract::Target::END]
    }

    fn evaluate(
        &self,
        _state: &br_graph_contract::State<Steps>,
    ) -> Result<Vec<br_graph_contract::Target>, br_graph_contract::Cause> {
        Ok(vec![br_graph_contract::Target::END])
    }
}

pub(crate) fn idle_plan() -> Arc<crate::plan::Plan<Steps>> {
    let graph = br_graph_contract::GraphBuilder::<Steps>::new("1")
        .entry("work")
        .node(Arc::new(Idle))
        .edge(Arc::new(ToEnd))
        .build()
        .expect("a graph");
    Arc::new(crate::plan::Plan {
        graph: Arc::new(graph),
        run_start: crate::run_start::run_start(|_input| async {
            Ok::<_, crate::failure::RunRefusal>(crate::run_start::StartedRun::new(
                Steps::default(),
                vec!["work".to_owned()],
            ))
        }),
        can_restart: false,
        state_limit: None,
        mapping: None,
        answer: None,
    })
}

pub(crate) fn started_facts(jobs: &Arc<ScriptedJobs>) -> usize {
    jobs.calls()
        .into_iter()
        .filter(|call| matches!(call, br_runner_jobs::test_double::Call::Started { .. }))
        .count()
}
