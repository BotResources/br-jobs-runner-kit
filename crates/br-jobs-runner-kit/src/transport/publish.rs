use std::time::Duration;

use async_nats::jetstream::Context;
use br_jobs_runner_core::Backoff;
use contract_jobs::runner_transport::{RunnerStatusFact, log_subject, status_subject};
use contract_jobs::segment::SubjectSegment;
use serde::Serialize;
use tokio::sync::mpsc;

use super::publish_acked;
use crate::facts::RunFact;

const STATUS_PUBLISH_ATTEMPTS: u32 = 8;

pub(crate) async fn publish_facts(
    js: Context,
    runner_type: SubjectSegment,
    mut facts: mpsc::UnboundedReceiver<RunFact>,
) {
    while let Some(fact) = facts.recv().await {
        match fact {
            RunFact::Log(line) => {
                let subject = log_subject(&runner_type);
                match serde_json::to_vec(&line) {
                    Ok(bytes) => {
                        if let Err(error) = js.publish(subject, bytes.into()).await {
                            tracing::debug!(%error, "a log line was dropped (best-effort stream)");
                        }
                    }
                    Err(error) => tracing::debug!(%error, "a log line failed to serialize"),
                }
            }
            RunFact::PlanDeclared(plan) => {
                publish_status(&js, &runner_type, RunnerStatusFact::PlanDeclared, &plan).await;
            }
            RunFact::StepStarted(step) => {
                publish_status(&js, &runner_type, RunnerStatusFact::StepStarted, &step).await;
            }
            RunFact::Completed(completed) => {
                publish_status(&js, &runner_type, RunnerStatusFact::Completed, &completed).await;
            }
            RunFact::Failed(failed) => {
                publish_status(&js, &runner_type, RunnerStatusFact::Failed, &failed).await;
            }
        }
    }
}

async fn publish_status<T: Serialize>(
    js: &Context,
    runner_type: &SubjectSegment,
    fact: RunnerStatusFact,
    payload: &T,
) {
    let subject = status_subject(runner_type, fact);
    let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(15))
        .expect("the publish backoff bounds are static and valid");
    for attempt in 1..=STATUS_PUBLISH_ATTEMPTS {
        match publish_acked(js, subject.clone(), payload).await {
            Ok(()) => return,
            Err(error) => {
                tracing::warn!(%subject, attempt, %error, "publishing a status fact failed");
            }
        }
        tokio::time::sleep(backoff.next_delay()).await;
    }
    tracing::error!(
        %subject,
        "a status fact was lost after {STATUS_PUBLISH_ATTEMPTS} attempts; the presence TTL is the server's backstop"
    );
}
