use std::num::NonZeroUsize;

use br_agentic_contract::{Cursor, IncomingBatch, IncomingContext, PortError};
use br_runner_jobs_client::AgentBook;
use futures::future::try_join_all;
use uuid::Uuid;

use super::convert::convert;
use super::listed::Listed;
use super::position::Position;
use super::resolve;
use crate::adapter::node_entry::Shared;

pub(super) async fn read_unread(book: &AgentBook, agent: Uuid) -> Result<Vec<Listed>, PortError> {
    book.core()
        .mail_handle()
        .unread_sequenced(agent)
        .await
        .map(|read| read.into_iter().map(Listed::from).collect())
        .map_err(PortError::fault)
}

pub(super) fn leave_out_marked(book: &AgentBook, agent: Uuid, items: &mut Vec<Listed>) {
    let marked = book.marked_items(agent);
    items.retain(|sequenced| !marked.contains(&sequenced.item.item_id()));
}

fn kept_rest(
    shared: &Shared,
    book: &AgentBook,
    agent: Uuid,
    after: Position,
) -> Option<Vec<Listed>> {
    let kept = shared.seqs.sequence(agent, book.snapshot(agent)?)?;
    kept.iter()
        .any(|item| item.item.item_id() == after.item_id)
        .then(|| after.rest_of(kept))
}

async fn rest_of_listing(
    shared: &Shared,
    book: &AgentBook,
    agent: Uuid,
    after: Option<Position>,
) -> Result<(Vec<Listed>, Option<Vec<Listed>>), PortError> {
    if let Some(after) = after
        && let Some(rest) = kept_rest(shared, book, agent, after)
    {
        return Ok((rest, None));
    }
    let read = read_unread(book, agent).await?;
    let rest = match after {
        None => read.clone(),
        Some(after) => after.rest_of(read.clone()),
    };
    Ok((rest, Some(read)))
}

pub(in crate::adapter) async fn items(
    shared: &Shared,
    context: &IncomingContext,
    after: Option<&Cursor>,
    limit: NonZeroUsize,
) -> Result<IncomingBatch, PortError> {
    let resolved = resolve(shared, context).await?;
    let (book, agent) = (&resolved.book, resolved.agent);
    let after = after.map(Position::parse).transpose()?;
    if book.registration_queued(agent) {
        return Ok(IncomingBatch {
            items: Vec::new(),
            next: None,
        });
    }
    let (mut rest, fresh) = rest_of_listing(shared, book, agent, after).await?;
    leave_out_marked(book, agent, &mut rest);
    let more = rest.len() > limit.get();
    rest.truncate(limit.get());
    let next = rest
        .last()
        .filter(|_| more)
        .map(|last| Position::of(last).cursor());
    let converted = try_join_all(
        rest.into_iter()
            .map(|sequenced| convert(book, agent, sequenced.item)),
    )
    .await?;
    match (next.is_some(), fresh) {
        (true, Some(read)) => {
            shared.seqs.keep(agent, &read);
            book.keep_snapshot(
                agent,
                read.into_iter().map(|sequenced| sequenced.item).collect(),
            );
        }
        (true, None) => {}
        (false, _) => {
            shared.seqs.drop_agent(agent);
            book.drop_snapshot(agent);
        }
    }
    let mut items = Vec::with_capacity(converted.len());
    for done in converted {
        book.record_listed(agent, done.item.id().as_uuid(), done.entry);
        items.push(done.item);
    }
    Ok(IncomingBatch { items, next })
}
