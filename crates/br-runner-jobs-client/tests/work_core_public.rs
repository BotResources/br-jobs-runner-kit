use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_runner_jobs_client::test_double::{Call, ScriptedJobs, trigger_json, work_from_trigger};
use br_runner_jobs_client::{IgnoreReports, QueueSettings, RunnerType, Work, WorkCore};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn work(jobs: &Arc<ScriptedJobs>) -> Work {
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

async fn core_of(work: &Work) -> Arc<WorkCore> {
    let (queue, started) = work
        .delivery_queue(QueueSettings {
            start_id: id(9),
            outcome_wait: Duration::from_secs(5),
            retry_wait: Duration::from_millis(10),
            observer: Arc::new(IgnoreReports),
        })
        .await
        .expect("a queue");
    started.await.expect("started");
    let book = work.agent_book(&queue, Vec::new(), false).expect("a book");
    Arc::clone(book.core())
}

fn jobs() -> Arc<ScriptedJobs> {
    ScriptedJobs::new(
        RunnerType::new("runner").expect("a runner type"),
        "instance-a",
    )
}

#[tokio::test]
async fn given_the_core_of_a_work_when_it_is_read_then_ids_and_run_are_the_ones_of_the_work() {
    let jobs = jobs();
    let work = work(&jobs);
    let core = core_of(&work).await;
    let ids = core.ids();
    assert_eq!(
        (ids.job_id, ids.run_id, ids.instance_key.as_str()),
        (id(1), id(2), "instance-a")
    );
    assert_eq!(core.run_id(), work.run_id());
}

#[tokio::test]
async fn given_the_core_of_a_work_when_its_mail_handle_reads_then_the_calls_go_through_the_backend_with_the_ids_of_the_work()
 {
    let jobs = jobs();
    let work = work(&jobs);
    let core = core_of(&work).await;
    let handle = core.mail_handle();
    assert!(handle.unread(id(70)).await.is_err());
    handle.mark_read(id(71)).await.expect("marked");
    let calls = jobs.calls();
    let graphql = calls
        .iter()
        .find_map(|call| match call {
            Call::Graphql { variables, .. } => Some(variables.clone()),
            _ => None,
        })
        .expect("a graphql call");
    let ids = core.ids();
    assert_eq!(
        graphql["work"],
        serde_json::json!({
            "jobId": ids.job_id,
            "runId": ids.run_id,
            "instanceKey": ids.instance_key,
        })
    );
    assert_eq!(graphql["address"], serde_json::json!(id(70)));
    assert!(calls.iter().any(|call| matches!(
        call,
        Call::MarkRead { run_id, item_ids } if *run_id == id(2) && item_ids == &vec![id(71)]
    )));
}
