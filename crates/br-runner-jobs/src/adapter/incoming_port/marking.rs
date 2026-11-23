use std::sync::Arc;

use br_agentic_contract::{IncomingContext, PortError};
use br_llm_messages::EntryId;
use br_runner_jobs_client::{AgentBook, CallNoticeKind, EntryKind, MailEntry, Write};

use super::resolve;
use crate::adapter::gate::MarkGate;
use crate::adapter::node_entry::Shared;

pub(in crate::adapter) struct Prepared {
    pub marker: Write,
    pub finish: Option<Write>,
}

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

pub(in crate::adapter) async fn prepare(
    shared: &Shared,
    context: &IncomingContext,
    entry: EntryId,
) -> Result<(Option<Prepared>, Arc<AgentBook>), PortError> {
    let resolved = resolve(shared, context).await?;
    let (book, agent) = (resolved.book, resolved.agent);
    let Some(listed) = book.listed(agent, entry.as_uuid()) else {
        return Ok((None, book));
    };
    if !book.mark(agent, listed.item_id) {
        return Ok((None, book));
    }
    let run_id = book.core().run_id();
    let prepared = Prepared {
        marker: Write::Marker {
            run_id,
            item_id: listed.item_id,
            agent_id: agent,
            entry_id: entry.as_uuid(),
        },
        finish: finish_of(run_id, &listed),
    };
    Ok((Some(prepared), book))
}

pub(super) async fn mark_read(
    shared: &Shared,
    gate: &MarkGate,
    context: &IncomingContext,
    entry: EntryId,
) -> Result<(), PortError> {
    let (prepared, book) = prepare(shared, context, entry).await?;
    let Some(prepared) = prepared else {
        return Ok(());
    };
    let gate = gate(context.thread, entry);
    drop(
        book.queue_writer()
            .hold(prepared.marker, prepared.finish, gate),
    );
    Ok(())
}
