use std::future::Future;
use std::sync::Arc;

use br_graph_contract::{CancelView, Fields};
use br_runner_jobs::Work;
use futures::future::BoxFuture;

use crate::context::RunContext;
use crate::failure::RunRefusal;

pub struct RunStartInput<F: Fields> {
    pub work: Arc<Work>,
    pub context: RunContext,
    pub cancel: CancelView,
    pub restored: Option<F>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StartedRun<F> {
    pub fields: F,
    pub first_nodes: Vec<String>,
}

impl<F> StartedRun<F> {
    pub fn new(fields: F, first_nodes: Vec<String>) -> Self {
        Self {
            fields,
            first_nodes,
        }
    }
}

pub trait RunStart<F: Fields>: Send + Sync {
    fn start(
        &self,
        input: RunStartInput<F>,
    ) -> BoxFuture<'static, Result<StartedRun<F>, RunRefusal>>;
}

struct FromFn<G>(G);

impl<F, G, Fut> RunStart<F> for FromFn<G>
where
    F: Fields,
    G: Fn(RunStartInput<F>) -> Fut + Send + Sync,
    Fut: Future<Output = Result<StartedRun<F>, RunRefusal>> + Send + 'static,
{
    fn start(
        &self,
        input: RunStartInput<F>,
    ) -> BoxFuture<'static, Result<StartedRun<F>, RunRefusal>> {
        Box::pin((self.0)(input))
    }
}

pub fn run_start<F, G, Fut>(function: G) -> Arc<dyn RunStart<F>>
where
    F: Fields,
    G: Fn(RunStartInput<F>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<StartedRun<F>, RunRefusal>> + Send + 'static,
{
    Arc::new(FromFn(function))
}
