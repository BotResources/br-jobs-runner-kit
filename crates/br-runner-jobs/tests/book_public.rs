use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_runner_jobs::test_double::{
    Call, ChildCommand, ScriptedJobs, trigger_json, work_from_trigger,
};
use br_runner_jobs::{
    AgentKey, CallMode, CreateChildInput, Error, IgnoreReports, PayloadForm, QueueSettings,
    RestoredAgent,
};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn settings() -> QueueSettings {
    QueueSettings {
        start_id: id(9),
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(10),
        observer: Arc::new(IgnoreReports),
    }
}

fn work(jobs: &Arc<ScriptedJobs>) -> br_runner_jobs::Work {
    let passport = Passport::human(
        id(5),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::new(),
    );
    let trigger = trigger_json(id(2), id(1), id(3), id(4), &passport).expect("a trigger");
    work_from_trigger(jobs, trigger).expect("a Work")
}

#[tokio::test]
async fn given_a_work_from_a_trigger_when_the_book_the_handle_and_a_create_are_used_through_the_public_api_then_jobs_receives_the_create_of_the_work()
 {
    let jobs = ScriptedJobs::new("runner", "instance-a");
    let work = work(&jobs);
    let (queue, started) = work.delivery_queue(settings()).await.expect("a queue");
    started.await.expect("started");
    let restored = RestoredAgent {
        key: AgentKey::new("node", "writer"),
        agent_id: id(70),
        has_inbox: true,
    };
    let book = work
        .agent_book(&queue, vec![restored], true)
        .expect("a book");
    assert_eq!(book.agents().len(), 1);
    let handle = book.caller_handle();
    let input = CreateChildInput::new(
        id(300),
        id(301),
        "analyst",
        CallMode::Sync,
        PayloadForm::Kept,
    );
    handle
        .create_child(input)
        .expect("handed over")
        .await
        .expect("delivered");
    let creates: Vec<_> = jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Command(ChildCommand::Create(create)) => Some(create),
            _ => None,
        })
        .collect();
    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0].parent_job_id, work.job_id());
    assert_eq!(creates[0].initial_job_id, work.initial_job_id());
    assert_eq!(creates[0].run_id, work.run_id());
}

#[tokio::test]
async fn given_a_book_made_when_a_second_is_made_from_the_same_work_then_it_is_invalid_input() {
    let jobs = ScriptedJobs::new("runner", "instance-a");
    let work = work(&jobs);
    let (queue, _) = work.delivery_queue(settings()).await.expect("a queue");
    work.agent_book(&queue, Vec::new(), false).expect("a book");
    assert!(matches!(
        work.agent_book(&queue, Vec::new(), false),
        Err(Error::InvalidInput(_))
    ));
}
