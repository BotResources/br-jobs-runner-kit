use chrono::Utc;
use contract_jobs::runner::{
    FailureKind as WireKind, LOG_STREAM, LogLine, PlanDeclared, RunCompleted, RunFailed,
    RunStarted, STATUS_STREAM, StepStarted,
};
use contract_jobs::runner_transport::{RunnerStatusFact, log_subject, status_subject};
use contract_jobs::segment::SubjectSegment;
use serde_json::json;
use uuid::Uuid;

use super::live;
use super::live_world::{WAIT, World, eventually};
use crate::backend::{Backend, Fact, LogRecord};
use crate::error::Error;
use crate::model::{Failed, FailureKind, LogLevel};

fn subject(fact: RunnerStatusFact) -> String {
    status_subject(
        &SubjectSegment::runner_type("analyst").expect("segment"),
        fact,
    )
}

async fn published(world: &World, fact: RunnerStatusFact) -> Vec<Vec<u8>> {
    let wanted = subject(fact);
    world
        .stored(STATUS_STREAM)
        .await
        .into_iter()
        .filter(|(subject, _)| *subject == wanted)
        .map(|(_, payload)| payload)
        .collect()
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_started_fact_when_published_then_the_status_stream_holds_the_wire_value_without_a_start_id()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    live.publish_fact(
        Fact::Started {
            run_id,
            start_id: Uuid::now_v7(),
        },
        Uuid::now_v7().to_string(),
        WAIT,
    )
    .await
    .map_err(|e| e.to_string())
    .expect("published");
    let held = published(&world, RunnerStatusFact::Started).await;
    assert_eq!(held.len(), 1);
    let started: RunStarted = serde_json::from_slice(&held[0]).expect("a RunStarted");
    assert_eq!(started.run_id, run_id);
    assert_eq!(started.instance_key, "pod-1");
    let raw: serde_json::Value = serde_json::from_slice(&held[0]).expect("json");
    assert!(raw.get("start_id").is_none());
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_plan_and_a_step_when_published_then_each_is_on_its_own_subject_with_its_fields() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    let declaration_id = Uuid::now_v7();
    let started_at = Utc::now();
    live.publish_fact(
        Fact::Plan {
            run_id,
            steps: vec!["read".to_owned(), "write".to_owned()],
            declaration_id: Some(declaration_id),
        },
        Uuid::now_v7().to_string(),
        WAIT,
    )
    .await
    .map_err(|e| e.to_string())
    .expect("published");
    live.publish_fact(
        Fact::Step {
            run_id,
            index: 1,
            label: "write".to_owned(),
            started_at,
        },
        Uuid::now_v7().to_string(),
        WAIT,
    )
    .await
    .map_err(|e| e.to_string())
    .expect("published");
    let plan: PlanDeclared =
        serde_json::from_slice(&published(&world, RunnerStatusFact::PlanDeclared).await[0])
            .expect("a plan");
    assert_eq!(plan.steps, vec!["read", "write"]);
    assert_eq!(plan.declaration_id, Some(declaration_id));
    assert_eq!(plan.run_id, run_id);
    let step: StepStarted =
        serde_json::from_slice(&published(&world, RunnerStatusFact::StepStarted).await[0])
            .expect("a step");
    assert_eq!(
        (step.index, step.label.as_str(), step.started_at),
        (1, "write", started_at)
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_completed_and_a_failed_fact_when_published_then_they_carry_the_kind_and_the_report()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    live.publish_fact(Fact::Completed { run_id }, Uuid::now_v7().to_string(), WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("published");
    live.publish_fact(
        Fact::Failed {
            run_id,
            failed: Failed {
                kind: FailureKind::Transient,
                reason_code: "upstream_down".to_owned(),
                params: json!({ "service": "kms" }),
                diagnostic: json!({ "detail": "503" }),
                retry_after_seconds: Some(30),
            },
        },
        Uuid::now_v7().to_string(),
        WAIT,
    )
    .await
    .map_err(|e| e.to_string())
    .expect("published");
    let completed: RunCompleted =
        serde_json::from_slice(&published(&world, RunnerStatusFact::Completed).await[0])
            .expect("completed");
    assert_eq!(completed.run_id, run_id);
    let failed: RunFailed =
        serde_json::from_slice(&published(&world, RunnerStatusFact::Failed).await[0])
            .expect("failed");
    assert_eq!(failed.report.kind, Some(WireKind::Transient));
    assert_eq!(failed.report.reason_code, "upstream_down");
    assert_eq!(failed.report.params, json!({ "service": "kms" }));
    assert_eq!(failed.report.diagnostic, json!({ "detail": "503" }));
    assert_eq!(failed.retry_after_seconds, Some(30));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_permanent_failure_without_a_hint_when_published_then_no_retry_hint_travels() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    live.publish_fact(
        Fact::Failed {
            run_id: Uuid::now_v7(),
            failed: Failed {
                kind: FailureKind::Permanent,
                reason_code: "bad_input".to_owned(),
                params: json!({}),
                diagnostic: json!(null),
                retry_after_seconds: None,
            },
        },
        Uuid::now_v7().to_string(),
        WAIT,
    )
    .await
    .map_err(|e| e.to_string())
    .expect("published");
    let failed: RunFailed =
        serde_json::from_slice(&published(&world, RunnerStatusFact::Failed).await[0])
            .expect("failed");
    assert_eq!(failed.report.kind, Some(WireKind::Permanent));
    assert_eq!(failed.retry_after_seconds, None);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_log_line_when_published_then_the_log_stream_holds_it_and_the_call_does_not_wait_for_the_broker()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let (run_id, id) = (Uuid::now_v7(), Uuid::now_v7());
    live.publish_log(LogRecord {
        id: Some(id),
        run_id,
        step_index: Some(2),
        level: LogLevel::Warning,
        message: "slow upstream".to_owned(),
        logged_at: Utc::now(),
    })
    .await
    .map_err(|e| e.to_string())
    .expect("sent");
    eventually("the log line is stored", || async {
        world.messages_in(LOG_STREAM).await == 1
    })
    .await;
    let wanted = log_subject(&SubjectSegment::runner_type("analyst").expect("segment"));
    let held = world.stored(LOG_STREAM).await;
    assert_eq!(held[0].0, wanted);
    let line: LogLine = serde_json::from_slice(&held[0].1).expect("a LogLine");
    assert_eq!(
        (line.id, line.run_id, line.step_index),
        (Some(id), run_id, Some(2))
    );
    assert_eq!(
        (line.level.as_str(), line.message.as_str()),
        ("WARNING", "slow upstream")
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_the_status_stream_gone_when_publishing_a_fact_then_infra_missing() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    world.delete_stream(STATUS_STREAM).await;
    let error = live
        .publish_fact(
            Fact::Completed {
                run_id: Uuid::now_v7(),
            },
            Uuid::now_v7().to_string(),
            WAIT,
        )
        .await
        .expect_err("refused");
    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_fact_published_twice_with_one_message_id_then_the_status_stream_holds_it_once() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let (run_id, message_id) = (Uuid::now_v7(), Uuid::now_v7().to_string());
    for _ in 0..2 {
        live.publish_fact(Fact::Completed { run_id }, message_id.clone(), WAIT)
            .await
            .map_err(|e| e.to_string())
            .expect("published");
    }
    assert_eq!(
        published(&world, RunnerStatusFact::Completed).await.len(),
        1
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_two_facts_of_one_kind_with_two_message_ids_then_the_status_stream_holds_both() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    for _ in 0..2 {
        live.publish_fact(Fact::Completed { run_id }, Uuid::now_v7().to_string(), WAIT)
            .await
            .map_err(|e| e.to_string())
            .expect("published");
    }
    assert_eq!(
        published(&world, RunnerStatusFact::Completed).await.len(),
        2
    );
}
