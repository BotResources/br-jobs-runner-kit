use std::future::Future;
use std::sync::Arc;

use br_graph_contract::InstanceId;
use br_runner_jobs::BookedAgent;
use futures::future::BoxFuture;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InstanceEnded {
    pub instance_id: InstanceId,
    pub node: String,
    pub agents: Vec<BookedAgent>,
}

pub type LifecycleHook = Arc<dyn Fn(InstanceEnded) -> BoxFuture<'static, ()> + Send + Sync>;

pub fn lifecycle_hook<F, Fut>(hook: F) -> LifecycleHook
where
    F: Fn(InstanceEnded) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Arc::new(move |ended| Box::pin(hook(ended)))
}
