use std::sync::Arc;

use br_agentic_contract::{InboxContext, PortError};
use futures::future::{Fuse, FutureExt, OptionFuture};
use uuid::Uuid;

use super::listing::{leave_out_marked, read_unread};
use super::resolve;
use crate::adapter::node_entry::Shared;
use crate::book::{AgentBook, RegistrationEnd, Wake};
use crate::error::Error;

async fn holds_an_item(book: &AgentBook, agent: Uuid) -> Result<bool, PortError> {
    if book.registration_queued(agent) {
        return Ok(false);
    }
    let mut items = read_unread(book, agent).await?;
    leave_out_marked(book, agent, &mut items);
    Ok(!items.is_empty())
}

fn ended(wake: Wake) -> Option<PortError> {
    match wake {
        Wake::Item(_) => None,
        Wake::FeedEnded(error) => Some(PortError::fault(error)),
        Wake::BookDropped => Some(PortError::fault(Error::BookDropped)),
    }
}

pub(super) async fn wait(shared: &Shared, context: &InboxContext) -> Result<(), PortError> {
    let resolved = resolve(shared, context).await?;
    let agent = resolved.agent;
    let mut watch = resolved.book.subscribe_items(agent);
    let weak = Arc::downgrade(&resolved.book);
    let queued = resolved
        .book
        .registration_queued(agent)
        .then(|| resolved.book.registration_end(agent))
        .flatten();
    let mut registration_open = queued.is_some();
    let mut registration: OptionFuture<Fuse<RegistrationEnd>> = queued.map(FutureExt::fuse).into();
    let mut strong = Some(resolved.book);
    let mut first = true;
    loop {
        let book = match strong.take().or_else(|| weak.upgrade()) {
            Some(book) => book,
            None => return Err(PortError::fault(Error::BookDropped)),
        };
        if registration_open && !book.registration_queued(agent) {
            registration_open = false;
        }
        if holds_an_item(&book, agent).await? {
            return Ok(());
        }
        if std::mem::take(&mut first)
            && let Some(error) = book.arrival_state()
        {
            return Err(PortError::fault(error));
        }
        drop(book);
        tokio::select! {
            wake = watch.next() => {
                if let Some(error) = ended(wake) {
                    return Err(error);
                }
            }
            _ = &mut registration, if registration_open => {
                registration_open = false;
            }
        }
        while let Some(wake) = watch.next().now_or_never() {
            if let Some(error) = ended(wake) {
                return Err(error);
            }
        }
    }
}
