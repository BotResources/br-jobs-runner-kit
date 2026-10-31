use std::future::Future;
use std::sync::Arc;

use br_agentic_contract::{Cause, PortFuture, ToolBlock};
use br_llm_messages::OpaqueJson;
use br_runner_jobs::Content;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Payload {
    Content(Content),
    Reference {
        service: String,
        document_id: String,
    },
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AuthorFailure {
    Refused(String),
    Fault(Cause),
}

impl AuthorFailure {
    pub fn refused(text: impl Into<String>) -> Self {
        Self::Refused(text.into())
    }

    pub fn fault(cause: impl Into<Cause>) -> Self {
        Self::Fault(cause.into())
    }
}

pub(crate) type PayloadBuilder =
    Arc<dyn Fn(OpaqueJson) -> PortFuture<'static, Result<Payload, AuthorFailure>> + Send + Sync>;

pub(crate) type ResultProcessor = Arc<
    dyn Fn(Content) -> PortFuture<'static, Result<Vec<ToolBlock>, AuthorFailure>> + Send + Sync,
>;

pub(crate) fn payload_builder<F, Fut>(function: F) -> PayloadBuilder
where
    F: Fn(OpaqueJson) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Payload, AuthorFailure>> + Send + 'static,
{
    Arc::new(move |arguments| Box::pin(function(arguments)))
}

pub(crate) fn result_processor<F, Fut>(function: F) -> ResultProcessor
where
    F: Fn(Content) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Vec<ToolBlock>, AuthorFailure>> + Send + 'static,
{
    Arc::new(move |result| Box::pin(function(result)))
}
