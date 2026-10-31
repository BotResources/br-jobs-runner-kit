use std::future::Future;
use std::sync::Arc;

use br_agentic_contract::{InstanceId, PortError, PortFuture};

use crate::book::AgentBook;

pub trait BookLookup: Send + Sync {
    fn book<'a>(
        &'a self,
        instance: InstanceId,
    ) -> PortFuture<'a, Result<Arc<AgentBook>, PortError>>;
}

struct FnLookup<F>(F);

impl<F, Fut> BookLookup for FnLookup<F>
where
    F: Fn(InstanceId) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Arc<AgentBook>, PortError>> + Send + 'static,
{
    fn book<'a>(
        &'a self,
        instance: InstanceId,
    ) -> PortFuture<'a, Result<Arc<AgentBook>, PortError>> {
        Box::pin((self.0)(instance))
    }
}

pub fn book_lookup<F, Fut>(lookup: F) -> Arc<dyn BookLookup>
where
    F: Fn(InstanceId) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Arc<AgentBook>, PortError>> + Send + 'static,
{
    Arc::new(FnLookup(lookup))
}
