mod support;

use std::sync::Arc;
use std::time::Duration;

use br_jobs_runner_kit::{HarnessError, RunContext, RunOutcome, Runner};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use uuid::Uuid;

use support::{
    PRESENCE_TTL, RunningHarness, StreamProbe, cancel_run, config, jetstream, presence_entry,
    provision, publish_trigger, wait_for_absence, wait_for_presence,
};

const FACT_TIMEOUT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_secs(2);

struct EchoRunner;

#[derive(serde::Deserialize)]
struct EchoPayload {
    text: String,
}

impl Runner for EchoRunner {
    type Payload = EchoPayload;

    async fn execute(&self, ctx: RunContext, payload: Self::Payload) -> RunOutcome {
        ctx.declare_plan(vec!["echo".to_owned()]);
        ctx.start_step(0, "echo");
        ctx.log(payload.text);
        RunOutcome::Completed
    }
}

struct FailingRunner;

impl Runner for FailingRunner {
    type Payload = Value;

    async fn execute(&self, _ctx: RunContext, _payload: Self::Payload) -> RunOutcome {
        RunOutcome::failed_retry_after("llm_unreachable", Duration::from_secs(60))
    }
}

struct GatedRunner {
    gate: Arc<Semaphore>,
}

impl Runner for GatedRunner {
    type Payload = Value;

    async fn execute(&self, ctx: RunContext, _payload: Self::Payload) -> RunOutcome {
        tokio::select! {
            permit = self.gate.acquire() => {
                permit.expect("the gate stays open").forget();
                RunOutcome::Completed
            }
            _ = ctx.cancelled() => RunOutcome::failed("cancelled"),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn presence_heartbeats_outlive_the_ttl_and_shutdown_deletes_the_entry() {
    let js = jetstream().await;
    provision(&js).await;

    let harness = RunningHarness::start(config("presence-rt", "pod-0"), EchoRunner);
    let entry = wait_for_presence(&js, "presence-rt", "pod-0").await;
    assert_eq!(entry["status"], "READY");
    assert_eq!(entry["capacity"], 1);
    assert_eq!(entry["runner_version"], "e2e-0.1.0");

    tokio::time::sleep(PRESENCE_TTL + Duration::from_secs(1)).await;
    assert!(
        presence_entry(&js, "presence-rt", "pod-0").await.is_some(),
        "the heartbeat must keep the entry alive past the bucket ttl"
    );

    harness.shut_down().await.expect("a clean drain");
    wait_for_absence(&js, "presence-rt", "pod-0").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_trigger_flows_through_started_plan_step_completed_and_logs() {
    let js = jetstream().await;
    provision(&js).await;
    let mut status = StreamProbe::on_status(&js, "echo-rt").await;
    let mut logs = StreamProbe::on_logs(&js, "echo-rt").await;

    let harness = RunningHarness::start(config("echo-rt", "pod-0"), EchoRunner);
    wait_for_presence(&js, "echo-rt", "pod-0").await;
    let run_id = Uuid::now_v7();
    publish_trigger(&js, "echo-rt", run_id, json!({ "text": "bonjour" })).await;

    let started = status.expect_fact("started", FACT_TIMEOUT).await;
    assert_eq!(started["run_id"], json!(run_id));
    assert_eq!(started["instance_key"], "pod-0");
    let plan = status.expect_fact("plan_declared", FACT_TIMEOUT).await;
    assert_eq!(plan["steps"], json!(["echo"]));
    let step = status.expect_fact("step_started", FACT_TIMEOUT).await;
    assert_eq!(
        (step["index"].as_u64(), step["label"].as_str()),
        (Some(0), Some("echo"))
    );
    let completed = status.expect_fact("completed", FACT_TIMEOUT).await;
    assert_eq!(completed["run_id"], json!(run_id));

    let (fact, line) = logs
        .next_fact(FACT_TIMEOUT)
        .await
        .expect("one log line reaches the log stream");
    assert_eq!(fact, "echo-rt");
    assert_eq!(line["message"], "bonjour");
    assert_eq!(line["level"], "info");

    harness.shut_down().await.expect("a clean drain");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_failed_run_reports_its_reason_code_and_retry_hint() {
    let js = jetstream().await;
    provision(&js).await;
    let mut status = StreamProbe::on_status(&js, "fail-rt").await;

    let harness = RunningHarness::start(config("fail-rt", "pod-0"), FailingRunner);
    wait_for_presence(&js, "fail-rt", "pod-0").await;
    let run_id = Uuid::now_v7();
    publish_trigger(&js, "fail-rt", run_id, json!({})).await;

    status.expect_fact("started", FACT_TIMEOUT).await;
    let failed = status.expect_fact("failed", FACT_TIMEOUT).await;
    assert_eq!(failed["run_id"], json!(run_id));
    assert_eq!(failed["report"]["reason_code"], "llm_unreachable");
    assert_eq!(failed["retry_after_seconds"], 60);

    harness.shut_down().await.expect("a clean drain");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_cancel_entry_interrupts_an_in_flight_run() {
    let js = jetstream().await;
    provision(&js).await;
    let mut status = StreamProbe::on_status(&js, "cancel-rt").await;

    let gate = Arc::new(Semaphore::new(0));
    let harness = RunningHarness::start(
        config("cancel-rt", "pod-0"),
        GatedRunner {
            gate: Arc::clone(&gate),
        },
    );
    wait_for_presence(&js, "cancel-rt", "pod-0").await;
    let run_id = Uuid::now_v7();
    publish_trigger(&js, "cancel-rt", run_id, json!({})).await;
    status.expect_fact("started", FACT_TIMEOUT).await;

    cancel_run(&js, run_id).await;

    let failed = status.expect_fact("failed", FACT_TIMEOUT).await;
    assert_eq!(failed["run_id"], json!(run_id));
    assert_eq!(failed["report"]["reason_code"], "cancelled");

    harness.shut_down().await.expect("a clean drain");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_trigger_cancelled_before_boot_never_starts() {
    let js = jetstream().await;
    provision(&js).await;
    let mut status = StreamProbe::on_status(&js, "precancel-rt").await;

    let run_id = Uuid::now_v7();
    cancel_run(&js, run_id).await;
    let harness = RunningHarness::start(
        config("precancel-rt", "pod-0"),
        GatedRunner {
            gate: Arc::new(Semaphore::new(0)),
        },
    );
    wait_for_presence(&js, "precancel-rt", "pod-0").await;
    publish_trigger(&js, "precancel-rt", run_id, json!({})).await;

    status.expect_quiet(QUIET).await;

    harness.shut_down().await.expect("a clean drain");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn capacity_one_serializes_two_triggers() {
    let js = jetstream().await;
    provision(&js).await;
    let mut status = StreamProbe::on_status(&js, "cap-rt").await;

    let gate = Arc::new(Semaphore::new(0));
    let harness = RunningHarness::start(
        config("cap-rt", "pod-0"),
        GatedRunner {
            gate: Arc::clone(&gate),
        },
    );
    wait_for_presence(&js, "cap-rt", "pod-0").await;
    publish_trigger(&js, "cap-rt", Uuid::now_v7(), json!({})).await;
    publish_trigger(&js, "cap-rt", Uuid::now_v7(), json!({})).await;

    status.expect_fact("started", FACT_TIMEOUT).await;
    status.expect_quiet(QUIET).await;

    gate.add_permits(1);
    let mut after_release = Vec::new();
    for _ in 0..2 {
        let (fact, _) = status
            .next_fact(FACT_TIMEOUT)
            .await
            .expect("the released slot produces the next facts");
        after_release.push(fact);
    }
    after_release.sort();
    assert_eq!(
        after_release,
        vec!["completed".to_owned(), "started".to_owned()]
    );

    gate.add_permits(1);
    status.expect_fact("completed", FACT_TIMEOUT).await;

    harness.shut_down().await.expect("a clean drain");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_missing_declared_stream_fails_loud_at_boot() {
    let js = jetstream().await;
    provision(&js).await;
    js.delete_stream(contract_jobs::runner::TRIGGER_STREAM)
        .await
        .expect("removing the trigger stream to simulate a misprovisioned environment");

    let harness = RunningHarness::start(config("loud-rt", "pod-0"), EchoRunner);

    let outcome = harness.outcome().await;
    assert!(
        matches!(outcome, Err(HarnessError::Infra { .. })),
        "expected a loud infra failure, got: {outcome:?}"
    );
}
