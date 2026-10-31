pub(super) mod convert;
mod documents;
mod listing;
mod marking;
mod notice_text;
mod sending;
mod waiting;

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

use br_agentic_contract::{
    Cursor, InboxBatch, InboxContext, InboxPort, MailDocument, MailId, NewMail, PortError,
    PortFuture,
};
use br_llm_messages::EntryId;
use uuid::Uuid;

use super::node_entry::Shared;
use crate::book::AgentBook;
use crate::error::Error;

#[derive(Clone)]
pub struct InboxPortAdapter {
    shared: Arc<Shared>,
}

impl InboxPortAdapter {
    pub(super) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

pub(super) struct Resolved {
    pub book: Arc<AgentBook>,
    pub agent: Uuid,
}

pub(super) async fn resolve(
    shared: &Shared,
    context: &InboxContext,
) -> Result<Resolved, PortError> {
    let book = shared.lookup.book(context.instance).await?;
    let agent = context.agent.as_uuid();
    let holds = shared
        .inbox_key()
        .is_some_and(|key| book.key_of(agent) == Some(key) && book.holds_inbox(agent));
    if !holds {
        return Err(PortError::fault(Error::invalid(
            "the agent does not hold the inbox of this node",
        )));
    }
    Ok(Resolved { book, agent })
}

impl InboxPort for InboxPortAdapter {
    fn items<'a>(
        &'a self,
        context: &'a InboxContext,
        after: Option<&'a Cursor>,
        limit: NonZeroUsize,
    ) -> PortFuture<'a, Result<InboxBatch, PortError>> {
        Box::pin(listing::items(&self.shared, context, after, limit))
    }

    fn wait<'a>(&'a self, context: &'a InboxContext) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(waiting::wait(&self.shared, context))
    }

    fn mark_read<'a>(
        &'a self,
        context: &'a InboxContext,
        item: EntryId,
    ) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(marking::mark_read(&self.shared, context, item))
    }

    fn send<'a>(
        &'a self,
        context: &'a InboxContext,
        mail: NewMail,
    ) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(sending::send(&self.shared, context, mail))
    }

    fn documents<'a>(
        &'a self,
        context: &'a InboxContext,
        mail: MailId,
    ) -> PortFuture<'a, Result<Vec<MailDocument>, PortError>> {
        Box::pin(documents::documents(&self.shared, context, mail))
    }
}

impl fmt::Debug for InboxPortAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InboxPortAdapter").finish_non_exhaustive()
    }
}
