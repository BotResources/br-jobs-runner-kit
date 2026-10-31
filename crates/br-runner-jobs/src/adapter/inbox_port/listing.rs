use std::num::NonZeroUsize;

use br_agentic_contract::{Cursor, InboxBatch, InboxContext, PortError};
use futures::future::try_join_all;
use uuid::Uuid;

use super::convert::convert;
use super::resolve;
use crate::adapter::node_entry::Shared;
use crate::book::AgentBook;
use crate::error::Error;
use crate::graphql;
use crate::model::UnreadItem;

pub(super) async fn read_unread(
    book: &AgentBook,
    agent: Uuid,
) -> Result<Vec<UnreadItem>, PortError> {
    let core = book.core();
    graphql::unread(core.backend.as_ref(), &core.ids(), Some(agent))
        .await
        .map_err(PortError::fault)
}

pub(super) fn leave_out_marked(book: &AgentBook, agent: Uuid, items: &mut Vec<UnreadItem>) {
    let marked = book.marked_items(agent);
    items.retain(|item| !marked.contains(&item.item_id()));
}

fn parse_cursor(cursor: &Cursor) -> Result<Uuid, PortError> {
    Uuid::parse_str(cursor.as_str()).map_err(|_| {
        PortError::fault(Error::invalid(
            "the cursor of a listing is not one that this inbox gave",
        ))
    })
}

fn after_item(items: &[UnreadItem], after: Uuid) -> Option<Vec<UnreadItem>> {
    items
        .iter()
        .position(|item| item.item_id() == after)
        .map(|position| items[position + 1..].to_vec())
}

fn later_than(items: &[UnreadItem], after: Uuid) -> Vec<UnreadItem> {
    items
        .iter()
        .filter(|item| item.item_id() > after)
        .cloned()
        .collect()
}

async fn rest_of_listing(
    book: &AgentBook,
    agent: Uuid,
    after: Option<Uuid>,
) -> Result<(Vec<UnreadItem>, Option<Vec<UnreadItem>>), PortError> {
    if let Some(after) = after
        && let Some(rest) = book
            .snapshot(agent)
            .and_then(|kept| after_item(&kept, after))
    {
        return Ok((rest, None));
    }
    let read = read_unread(book, agent).await?;
    let rest = match after {
        None => read.clone(),
        Some(after) => after_item(&read, after).unwrap_or_else(|| later_than(&read, after)),
    };
    Ok((rest, Some(read)))
}

pub(super) async fn items(
    shared: &Shared,
    context: &InboxContext,
    after: Option<&Cursor>,
    limit: NonZeroUsize,
) -> Result<InboxBatch, PortError> {
    let resolved = resolve(shared, context).await?;
    let (book, agent) = (&resolved.book, resolved.agent);
    let after = after.map(parse_cursor).transpose()?;
    if book.registration_queued(agent) {
        return Ok(InboxBatch {
            items: Vec::new(),
            next: None,
        });
    }
    let (mut rest, fresh) = rest_of_listing(book, agent, after).await?;
    leave_out_marked(book, agent, &mut rest);
    let more = rest.len() > limit.get();
    rest.truncate(limit.get());
    let next = rest
        .last()
        .filter(|_| more)
        .map(|last| Cursor::new(last.item_id().to_string()));
    let converted = try_join_all(rest.into_iter().map(|item| convert(book, agent, item))).await?;
    match (next.is_some(), fresh) {
        (true, Some(read)) => book.keep_snapshot(agent, read),
        (true, None) => {}
        (false, _) => book.drop_snapshot(agent),
    }
    let mut items = Vec::with_capacity(converted.len());
    for done in converted {
        book.record_listed(agent, done.item.id().as_uuid(), done.entry);
        items.push(done.item);
    }
    Ok(InboxBatch { items, next })
}
