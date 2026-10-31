use std::sync::Arc;

use br_llm_messages::{EntryId, ResponseDelta, TurnEntry, TurnEntryKind};
use contract_threads::Chunk;

use super::shared::Core;
use crate::annotation::Annotation;
use crate::error::Error;

pub(crate) fn delta_chunk(message: uuid::Uuid, delta: &ResponseDelta) -> Result<Chunk, Error> {
    if delta.turn().as_uuid() != message {
        return Err(Error::invalid("a delta names another AI turn"));
    }
    Chunk::delta(delta).map_err(|e| Error::invalid(e.to_string()))
}

pub(crate) fn entry_chunk(entry: &TurnEntry) -> Result<Chunk, Error> {
    if matches!(
        entry.kind(),
        TurnEntryKind::ModelResponse | TurnEntryKind::TurnClosure
    ) {
        return Err(Error::invalid(
            "an entry chunk is neither a model response nor a closure",
        ));
    }
    Chunk::entry(entry).map_err(|e| Error::invalid(e.to_string()))
}

pub(crate) fn annotation_chunk(after: EntryId, annotation: &Annotation) -> Result<Chunk, Error> {
    Ok(Chunk::annotation(after.as_uuid(), annotation.to_wire()?))
}

pub(crate) async fn delta(core: &Arc<Core>, delta: &ResponseDelta) -> Result<(), Error> {
    let message = core.message_id;
    core.push(|| delta_chunk(message, delta)).await
}

pub(crate) async fn entry(core: &Arc<Core>, entry: &TurnEntry) -> Result<(), Error> {
    core.push(|| entry_chunk(entry)).await
}

pub(crate) async fn annotation(
    core: &Arc<Core>,
    after: EntryId,
    annotation: &Annotation,
) -> Result<(), Error> {
    core.push(|| annotation_chunk(after, annotation)).await
}

pub(crate) async fn keepalive(core: &Arc<Core>) -> Result<(), Error> {
    core.push(|| Ok(Chunk::Keepalive)).await
}
