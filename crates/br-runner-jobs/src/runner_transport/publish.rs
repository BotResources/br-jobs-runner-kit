use std::time::Duration;

use async_nats::jetstream::message::PublishMessage;
use contract_jobs::runner::{
    FailureKind as WireKind, FailureReport, LogLine, PlanDeclared, RunCompleted, RunFailed,
    RunStarted, StepStarted, WIRE_VERSION,
};
use contract_jobs::runner_transport::{RunnerStatusFact, log_subject, status_subject};
use serde::Serialize;
use tokio::time::{Instant, timeout_at};

use super::{Live, fail};
use crate::backend::{Fact, LogRecord};
use crate::error::{Error, Failure};
use crate::model::FailureKind;

fn body<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(value).map_err(|e| Error::invalid(e.to_string()))
}

fn wire_kind(kind: FailureKind) -> WireKind {
    match kind {
        FailureKind::Transient => WireKind::Transient,
        FailureKind::Permanent => WireKind::Permanent,
    }
}

impl Live {
    fn encode_fact(&self, fact: &Fact) -> Result<(String, Vec<u8>), Error> {
        let runner_type = &self.segments.runner_type;
        let (kind, bytes) = match fact {
            Fact::Started { run_id, .. } => (
                RunnerStatusFact::Started,
                body(&RunStarted {
                    version: WIRE_VERSION,
                    run_id: *run_id,
                    instance_key: self.instance_key.clone(),
                })?,
            ),
            Fact::Plan {
                run_id,
                steps,
                declaration_id,
            } => (
                RunnerStatusFact::PlanDeclared,
                body(&PlanDeclared {
                    version: WIRE_VERSION,
                    run_id: *run_id,
                    steps: steps.clone(),
                    declaration_id: *declaration_id,
                })?,
            ),
            Fact::Step {
                run_id,
                index,
                label,
                started_at,
            } => (
                RunnerStatusFact::StepStarted,
                body(&StepStarted {
                    version: WIRE_VERSION,
                    run_id: *run_id,
                    index: *index,
                    label: label.clone(),
                    started_at: *started_at,
                })?,
            ),
            Fact::Completed { run_id } => (
                RunnerStatusFact::Completed,
                body(&RunCompleted {
                    version: WIRE_VERSION,
                    run_id: *run_id,
                })?,
            ),
            Fact::Failed { run_id, failed } => (
                RunnerStatusFact::Failed,
                body(&RunFailed {
                    version: WIRE_VERSION,
                    run_id: *run_id,
                    report: FailureReport {
                        kind: Some(wire_kind(failed.kind)),
                        reason_code: failed.reason_code.clone(),
                        params: failed.params.clone(),
                        diagnostic: failed.diagnostic.clone(),
                    },
                    retry_after_seconds: failed.retry_after_seconds,
                })?,
            ),
        };
        Ok((status_subject(runner_type, kind), bytes))
    }

    pub(super) async fn fact(
        &self,
        fact: Fact,
        message_id: String,
        wait: Duration,
    ) -> Result<(), Error> {
        let (subject, bytes) = self.encode_fact(&fact)?;
        let object = "the status stream";
        let deadline = Instant::now() + wait;
        let message = PublishMessage::build()
            .payload(bytes.into())
            .message_id(message_id);
        let pending = timeout_at(deadline, self.wire.jetstream.send_publish(subject, message))
            .await
            .map_err(|_| Error::Unavailable(Failure::wait("the fact was not handed to the bus")))?
            .map_err(|e| fail::unsent(object, &e))?;
        timeout_at(deadline, pending)
            .await
            .map_err(|_| Error::wait_fired("the status stream did not confirm the fact"))?
            .map(drop)
            .map_err(|e| fail::sent(object, &e))
    }

    pub(super) async fn log(&self, line: LogRecord) -> Result<(), Error> {
        let bytes = body(&LogLine {
            version: WIRE_VERSION,
            id: line.id,
            run_id: line.run_id,
            step_index: line.step_index,
            level: line.level.as_str().to_owned(),
            message: line.message,
            logged_at: line.logged_at,
        })?;
        self.connection
            .bus()
            .publish(log_subject(&self.segments.runner_type), bytes.into())
            .await
            .map_err(|e| fail::unsent("the log line", &e))
    }
}
