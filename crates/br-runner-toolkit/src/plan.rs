use std::num::NonZeroUsize;
use std::sync::Arc;

use br_graph_contract::{Fields, Graph, State};
use br_runner_jobs::Content;

use crate::failure::FailureMapping;
use crate::run_start::RunStart;

pub type AnswerFn<F> = Arc<dyn Fn(&State<F>) -> Result<Content, String> + Send + Sync>;

pub(crate) struct Plan<F: Fields> {
    pub graph: Arc<Graph<F>>,
    pub run_start: Arc<dyn RunStart<F>>,
    pub can_restart: bool,
    pub state_limit: Option<NonZeroUsize>,
    pub mapping: Option<Arc<dyn FailureMapping>>,
    pub answer: Option<AnswerFn<F>>,
}
