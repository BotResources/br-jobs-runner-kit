use chrono::Utc;
use contract_jobs::runner::{LogLine, PlanDeclared, StepStarted, WIRE_VERSION};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::facts::RunFact;

pub const LOG_LEVEL_INFO: &str = "INFO";
pub const LOG_LEVEL_WARNING: &str = "WARNING";
pub const LOG_LEVEL_ERROR: &str = "ERROR";

#[derive(Debug, Clone)]
pub struct RunContext {
    run_id: Uuid,
    job_id: Uuid,
    attempt: u32,
    facts: mpsc::UnboundedSender<RunFact>,
    cancellation: CancellationToken,
}

impl RunContext {
    pub(crate) fn new(
        run_id: Uuid,
        job_id: Uuid,
        attempt: u32,
        facts: mpsc::UnboundedSender<RunFact>,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            run_id,
            job_id,
            attempt,
            facts,
            cancellation,
        }
    }

    pub fn stub(run_id: Uuid) -> (Self, mpsc::UnboundedReceiver<RunFact>, CancellationToken) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let cancellation = CancellationToken::new();
        (
            Self::new(run_id, run_id, 1, sender, cancellation.clone()),
            receiver,
            cancellation,
        )
    }

    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    pub fn job_id(&self) -> Uuid {
        self.job_id
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn declare_plan(&self, steps: Vec<String>) {
        self.emit(RunFact::PlanDeclared(PlanDeclared {
            version: WIRE_VERSION,
            run_id: self.run_id,
            steps,
            declaration_id: Some(Uuid::now_v7()),
        }));
    }

    pub fn start_step(&self, index: u32, label: impl Into<String>) {
        self.emit(RunFact::StepStarted(StepStarted {
            version: WIRE_VERSION,
            run_id: self.run_id,
            index,
            label: label.into(),
            started_at: Utc::now(),
        }));
    }

    pub fn log(&self, message: impl Into<String>) {
        self.log_with(LOG_LEVEL_INFO, None, message);
    }

    pub fn log_with(
        &self,
        level: impl Into<String>,
        step_index: Option<u32>,
        message: impl Into<String>,
    ) {
        let declared_level = level.into();
        let Some(level) = canonical_log_level(&declared_level) else {
            tracing::warn!(
                run_id = %self.run_id,
                level = %declared_level,
                "run log line has an unsupported level; expected INFO, WARNING or ERROR"
            );
            return;
        };
        self.emit(RunFact::Log(LogLine {
            version: WIRE_VERSION,
            id: Some(Uuid::now_v7()),
            run_id: self.run_id,
            step_index,
            level: level.to_owned(),
            message: message.into(),
            logged_at: Utc::now(),
        }));
    }

    pub async fn cancelled(&self) {
        self.cancellation.cancelled().await;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    fn emit(&self, fact: RunFact) {
        let _ = self.facts.send(fact);
    }
}

fn canonical_log_level(level: &str) -> Option<&'static str> {
    match level.trim().to_ascii_uppercase().as_str() {
        LOG_LEVEL_INFO => Some(LOG_LEVEL_INFO),
        "WARN" | LOG_LEVEL_WARNING => Some(LOG_LEVEL_WARNING),
        LOG_LEVEL_ERROR => Some(LOG_LEVEL_ERROR),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_context_when_reporting_then_wire_facts_arrive_in_order() {
        let run_id = Uuid::now_v7();
        let (ctx, mut facts, _cancel) = RunContext::stub(run_id);

        ctx.declare_plan(vec!["fetch".to_owned()]);
        ctx.start_step(0, "fetch");
        ctx.log("fetching");

        let RunFact::PlanDeclared(plan) = facts.try_recv().expect("plan fact") else {
            panic!("expected the plan first");
        };
        assert_eq!(plan.run_id, run_id);
        assert_eq!(plan.steps, vec!["fetch".to_owned()]);
        assert!(plan.declaration_id.is_some());

        let RunFact::StepStarted(step) = facts.try_recv().expect("step fact") else {
            panic!("expected the step second");
        };
        assert_eq!((step.index, step.label.as_str()), (0, "fetch"));

        let RunFact::Log(line) = facts.try_recv().expect("log fact") else {
            panic!("expected the log third");
        };
        assert_eq!(line.level, LOG_LEVEL_INFO);
        assert_eq!(line.message, "fetching");
        assert!(line.id.is_some());
    }

    #[test]
    fn given_leveled_log_when_reporting_then_level_and_step_carry_through() {
        let (ctx, mut facts, _cancel) = RunContext::stub(Uuid::now_v7());

        ctx.log_with("error", Some(2), "boom");

        let RunFact::Log(line) = facts.try_recv().expect("log fact") else {
            panic!("expected a log fact");
        };
        assert_eq!(
            (line.level.as_str(), line.step_index),
            (LOG_LEVEL_ERROR, Some(2))
        );
    }

    #[test]
    fn given_warn_alias_when_reporting_then_wire_level_is_warning() {
        let (ctx, mut facts, _cancel) = RunContext::stub(Uuid::now_v7());

        ctx.log_with("warn", None, "nearly out of retries");

        let RunFact::Log(line) = facts.try_recv().expect("a log fact was emitted") else {
            panic!("expected a log fact");
        };
        assert_eq!(line.level, LOG_LEVEL_WARNING);
    }

    #[test]
    fn given_unsupported_level_when_reporting_then_invalid_wire_fact_is_not_emitted() {
        let (ctx, mut facts, _cancel) = RunContext::stub(Uuid::now_v7());

        ctx.log_with("debug", None, "internal detail");

        assert!(matches!(
            facts.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn given_cancel_token_when_fired_then_context_observes_cancellation() {
        let (ctx, _facts, cancel) = RunContext::stub(Uuid::now_v7());
        assert!(!ctx.is_cancelled());

        cancel.cancel();

        assert!(ctx.is_cancelled());
        ctx.cancelled().await;
    }

    #[test]
    fn given_dropped_receiver_when_reporting_then_no_panic() {
        let (ctx, facts, _cancel) = RunContext::stub(Uuid::now_v7());
        drop(facts);
        ctx.log("into the void");
    }
}
