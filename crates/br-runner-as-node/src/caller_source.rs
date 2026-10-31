use std::future::Future;
use std::pin::Pin;

use br_graph_contract::{Cause, InstanceId};
use br_runner_jobs::CallerHandle;

pub type CallerFuture = Pin<Box<dyn Future<Output = Result<CallerHandle, Cause>> + Send>>;

pub trait CallerSource: Send + Sync + 'static {
    fn caller(&self, id: InstanceId) -> CallerFuture;
}

impl<S, Fut> CallerSource for S
where
    S: Fn(InstanceId) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<CallerHandle, Cause>> + Send + 'static,
{
    fn caller(&self, id: InstanceId) -> CallerFuture {
        Box::pin(self(id))
    }
}
