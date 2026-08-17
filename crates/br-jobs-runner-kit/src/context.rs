use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::facts::RunFact;

#[derive(Debug, Clone)]
pub struct RunContext {
    run_id: Uuid,
    facts: mpsc::UnboundedSender<RunFact>,
    cancellation: CancellationToken,
}

impl RunContext {
    pub(crate) fn new(
        run_id: Uuid,
        facts: mpsc::UnboundedSender<RunFact>,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            run_id,
            facts,
            cancellation,
        }
    }

    pub fn stub(run_id: Uuid) -> (Self, mpsc::UnboundedReceiver<RunFact>, CancellationToken) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let cancellation = CancellationToken::new();
        (
            Self::new(run_id, sender, cancellation.clone()),
            receiver,
            cancellation,
        )
    }

    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    pub fn declare_plan(&self, steps: Vec<String>) {
        self.emit(RunFact::PlanDeclared {
            run_id: self.run_id,
            steps,
        });
    }

    pub fn start_step(&self, index: u32) {
        self.emit(RunFact::StepStarted {
            run_id: self.run_id,
            index,
        });
    }

    pub fn log(&self, line: impl Into<String>) {
        self.emit(RunFact::Log {
            run_id: self.run_id,
            line: line.into(),
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_context_when_reporting_then_facts_arrive_in_order() {
        let run_id = Uuid::now_v7();
        let (ctx, mut facts, _cancel) = RunContext::stub(run_id);

        ctx.declare_plan(vec!["fetch".to_owned(), "summarize".to_owned()]);
        ctx.start_step(0);
        ctx.log("fetching");

        assert_eq!(
            facts.try_recv().expect("plan fact"),
            RunFact::PlanDeclared {
                run_id,
                steps: vec!["fetch".to_owned(), "summarize".to_owned()]
            }
        );
        assert_eq!(
            facts.try_recv().expect("step fact"),
            RunFact::StepStarted { run_id, index: 0 }
        );
        assert_eq!(
            facts.try_recv().expect("log fact"),
            RunFact::Log {
                run_id,
                line: "fetching".to_owned()
            }
        );
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
