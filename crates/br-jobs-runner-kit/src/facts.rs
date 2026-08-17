use uuid::Uuid;

use crate::runner::FailureReport;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunFact {
    Started { run_id: Uuid },
    PlanDeclared { run_id: Uuid, steps: Vec<String> },
    StepStarted { run_id: Uuid, index: u32 },
    Log { run_id: Uuid, line: String },
    Completed { run_id: Uuid },
    Failed { run_id: Uuid, report: FailureReport },
}
