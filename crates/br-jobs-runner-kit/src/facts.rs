use contract_jobs::runner::{LogLine, PlanDeclared, RunCompleted, RunFailed, StepStarted};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunFact {
    PlanDeclared(PlanDeclared),
    StepStarted(StepStarted),
    Log(LogLine),
    Completed(RunCompleted),
    Failed(RunFailed),
}
