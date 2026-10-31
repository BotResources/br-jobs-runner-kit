use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use br_graph_contract::{
    CancelView, Cause, CreateFuture, Fields, Instance, InstanceId, Node, PROGRAMMATIC, State,
};
use contract_jobs::segment::SubjectSegment;

use crate::caller_source::CallerSource;
use crate::instance;
use crate::outcome::Outcome;
use crate::spec::JobSpec;

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

type RequestFn<F> = dyn Fn(Arc<State<F>>) -> BoxFuture<Result<JobSpec, Cause>> + Send + Sync;

type ResponseFn<F> =
    dyn Fn(Arc<State<F>>, Outcome) -> BoxFuture<Result<<F as Fields>::Update, Cause>> + Send + Sync;

pub(crate) struct Parts<F: Fields> {
    pub(crate) runner_type: SubjectSegment,
    pub(crate) request: Box<RequestFn<F>>,
    pub(crate) response: Box<ResponseFn<F>>,
    pub(crate) caller_source: Box<dyn CallerSource>,
}

pub struct RunnerAsNode<F: Fields> {
    name: String,
    parts: Arc<Parts<F>>,
    max_attempts: Option<i64>,
}

impl<F: Fields> RunnerAsNode<F> {
    pub fn new<Req, ReqFut, Res, ResFut>(
        name: impl Into<String>,
        runner_type: SubjectSegment,
        request: Req,
        response: Res,
        caller_source: impl CallerSource,
    ) -> Self
    where
        Req: Fn(Arc<State<F>>) -> ReqFut + Send + Sync + 'static,
        ReqFut: Future<Output = Result<JobSpec, Cause>> + Send + 'static,
        Res: Fn(Arc<State<F>>, Outcome) -> ResFut + Send + Sync + 'static,
        ResFut: Future<Output = Result<F::Update, Cause>> + Send + 'static,
    {
        Self {
            name: name.into(),
            parts: Arc::new(Parts {
                runner_type,
                request: Box::new(move |state| Box::pin(request(state))),
                response: Box::new(move |state, outcome| Box::pin(response(state, outcome))),
                caller_source: Box::new(caller_source),
            }),
            max_attempts: None,
        }
    }

    #[must_use]
    pub fn with_max_attempts(mut self, max_attempts: i64) -> Self {
        self.max_attempts = Some(max_attempts);
        self
    }
}

impl<F: Fields> Node<F> for RunnerAsNode<F> {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> &str {
        PROGRAMMATIC
    }

    fn subkind(&self) -> &str {
        "runner_call"
    }

    fn create(&self, cancel: CancelView, id: InstanceId) -> CreateFuture<'_, F> {
        let parts = Arc::clone(&self.parts);
        let max_attempts = self.max_attempts;
        Box::pin(async move {
            let (instance, reader, sender) = Instance::channel(id);
            tokio::spawn(instance::run(
                parts,
                max_attempts,
                id,
                reader,
                sender,
                cancel,
            ));
            Ok(instance)
        })
    }
}
