pub(super) mod convert;
pub(super) mod listing;
pub(super) mod marking;
mod notice_text;

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

use br_agentic_contract::{
    Cursor, IncomingBatch, IncomingContext, IncomingPort, PortError, PortFuture,
};
use br_llm_messages::EntryId;
use br_runner_jobs_client::{AgentBook, Error};
use uuid::Uuid;

use super::gate::MarkGate;
use super::node_entry::Shared;

#[derive(Clone)]
pub struct IncomingPortAdapter {
    shared: Arc<Shared>,
    gate: MarkGate,
}

impl IncomingPortAdapter {
    pub(super) fn new(shared: Arc<Shared>, gate: MarkGate) -> Self {
        Self { shared, gate }
    }
}

pub(in crate::adapter) struct Resolved {
    pub book: Arc<AgentBook>,
    pub agent: Uuid,
}

pub(in crate::adapter) async fn resolve(
    shared: &Shared,
    context: &IncomingContext,
) -> Result<Resolved, PortError> {
    let book = shared.lookup.book(context.instance).await?;
    let agent = context.agent.as_uuid();
    let holds = shared
        .inbox_key()
        .is_some_and(|key| book.key_of(agent) == Some(key) && book.holds_inbox(agent));
    if !holds {
        return Err(PortError::fault(Error::InvalidInput(
            "the agent does not hold the inbox of this node".to_owned(),
        )));
    }
    Ok(Resolved { book, agent })
}

impl IncomingPort for IncomingPortAdapter {
    fn items<'a>(
        &'a self,
        context: &'a IncomingContext,
        after: Option<&'a Cursor>,
        limit: NonZeroUsize,
    ) -> PortFuture<'a, Result<IncomingBatch, PortError>> {
        Box::pin(listing::items(&self.shared, context, after, limit))
    }

    fn mark_read<'a>(
        &'a self,
        context: &'a IncomingContext,
        item: EntryId,
    ) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(marking::mark_read(&self.shared, &self.gate, context, item))
    }
}

impl fmt::Debug for IncomingPortAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IncomingPortAdapter")
            .finish_non_exhaustive()
    }
}
