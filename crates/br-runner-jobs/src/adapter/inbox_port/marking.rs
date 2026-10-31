use br_agentic_contract::{InboxContext, PortError};
use br_llm_messages::EntryId;

use super::resolve;
use crate::adapter::node_entry::Shared;
use crate::book::{EntryKind, MailEntry};
use crate::model::CallNoticeKind;
use crate::queue::Write;

fn finish_of(run_id: uuid::Uuid, listed: &MailEntry) -> Option<Write> {
    match (listed.kind, listed.child_job_id) {
        (EntryKind::CallNotice(CallNoticeKind::RunCompleted), Some(job_id)) => {
            Some(Write::Finish {
                run_id,
                job_id,
                command_id: listed.item_id,
            })
        }
        _ => None,
    }
}

pub(super) async fn mark_read(
    shared: &Shared,
    context: &InboxContext,
    entry: EntryId,
) -> Result<(), PortError> {
    let resolved = resolve(shared, context).await?;
    let (book, agent) = (&resolved.book, resolved.agent);
    let Some(listed) = book.listed(agent, entry.as_uuid()) else {
        return Ok(());
    };
    if !book.mark(agent, listed.item_id) {
        return Ok(());
    }
    let Some(inbox) = shared.inbox.as_ref() else {
        return Ok(());
    };
    let run_id = book.core().run_id;
    let marker = Write::Marker {
        run_id,
        item_id: listed.item_id,
        agent_id: agent,
        entry_id: entry.as_uuid(),
    };
    let gate = (inbox.gate)(context.thread, entry);
    drop(
        book.queue_writer()
            .hold(marker, finish_of(run_id, &listed), gate),
    );
    Ok(())
}
