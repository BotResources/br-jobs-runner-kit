mod documents;
mod marking;
mod sending;

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

use br_agentic_contract::{
    Cursor, IncomingBatch, IncomingContext, MailDocument, MailId, NewMail, PortError, PortFuture,
};
use br_agentic_harness_contract::MailPort;
use br_llm_messages::EntryId;

use super::incoming_port::listing;
use super::node_entry::Shared;

#[derive(Clone)]
pub struct MailPortAdapter {
    shared: Arc<Shared>,
}

impl MailPortAdapter {
    pub(super) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

impl MailPort for MailPortAdapter {
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
        Box::pin(marking::mark_read(&self.shared, context, item))
    }

    fn send<'a>(
        &'a self,
        context: &'a IncomingContext,
        mail: NewMail,
    ) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(sending::send(&self.shared, context, mail))
    }

    fn documents<'a>(
        &'a self,
        context: &'a IncomingContext,
        mail: MailId,
    ) -> PortFuture<'a, Result<Vec<MailDocument>, PortError>> {
        Box::pin(documents::documents(&self.shared, context, mail))
    }
}

impl fmt::Debug for MailPortAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailPortAdapter").finish_non_exhaustive()
    }
}
