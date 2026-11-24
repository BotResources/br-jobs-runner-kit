use br_agentic_contract::{IncomingContext, PortError};
use br_llm_messages::EntryId;

use crate::adapter::incoming_port::marking::prepare;
use crate::adapter::node_entry::Shared;

pub(super) async fn mark_read(
    shared: &Shared,
    context: &IncomingContext,
    entry: EntryId,
) -> Result<(), PortError> {
    let (prepared, book) = prepare(shared, context, entry).await?;
    let Some(prepared) = prepared else {
        return Ok(());
    };
    let writer = book.queue_writer();
    if let Some(finish) = prepared.finish {
        drop(writer.hand_over(finish));
    }
    drop(writer.hand_over(prepared.marker));
    Ok(())
}
