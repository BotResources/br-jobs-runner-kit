use br_llm_messages::{
    ContentBlock, DeltaOp, Finalization, ModelResponse, ResponseDelta, ResponseDraft,
    ResponseMetadataUpdate, SlotLifecycle,
};

use crate::error::Error;

fn unreachable_response() -> Error {
    Error::invalid("the whole response cannot be reached by more deltas from what was sent")
}

fn metadata_update(
    draft: &ResponseDraft,
    response: &ModelResponse,
) -> Option<ResponseMetadataUpdate> {
    let held = draft.metadata();
    let mut update = ResponseMetadataUpdate::new();
    let mut changed = false;
    if let Some(usage) = response.usage().filter(|u| Some(*u) != held.usage()) {
        update = update.with_usage(usage.clone());
        changed = true;
    }
    if let Some(model) = response.model().filter(|m| Some(*m) != held.model()) {
        update = update.with_model(model.clone());
        changed = true;
    }
    if let Some(id) = response
        .provider_response_id()
        .filter(|i| Some(*i) != held.provider_response_id())
    {
        update = update.with_provider_response_id(id.clone());
        changed = true;
    }
    changed.then_some(update)
}

fn wrap(draft: &ResponseDraft, op: DeltaOp) -> ResponseDelta {
    ResponseDelta::new(draft.turn(), draft.entry(), op)
}

pub(crate) fn whole_response_deltas(
    draft: &ResponseDraft,
    response: &ModelResponse,
) -> Result<Vec<ResponseDelta>, Error> {
    let blocks: &[ContentBlock] = response.blocks();
    let slots = draft.view().slots();
    let mut ops = Vec::new();
    if slots.is_empty() && matches!(draft.phase(), br_llm_messages::DraftPhase::Unstarted { .. }) {
        ops.push(DeltaOp::response_started(response.metadata()));
        ops.extend(blocks.iter().cloned().map(DeltaOp::block_inserted));
    } else {
        if slots.len() > blocks.len() {
            return Err(unreachable_response());
        }
        if let Some(update) = metadata_update(draft, response) {
            ops.push(DeltaOp::response_metadata_updated(update));
        }
        for (slot, block) in slots.iter().zip(blocks) {
            if slot.block() != block.id() {
                return Err(unreachable_response());
            }
            if slot.lifecycle() == SlotLifecycle::Open {
                ops.push(DeltaOp::block_finalized(
                    block.id(),
                    Finalization::authoritative(block.clone()),
                ));
            }
        }
        ops.extend(
            blocks[slots.len()..]
                .iter()
                .cloned()
                .map(DeltaOp::block_inserted),
        );
    }
    ops.push(DeltaOp::response_completed(response.ending().clone()));
    let deltas: Vec<ResponseDelta> = ops.into_iter().map(|op| wrap(draft, op)).collect();
    let mut fold = draft.clone();
    for delta in &deltas {
        fold.apply(delta).map_err(|_| unreachable_response())?;
    }
    if fold.into_response().as_ref() != Some(response) {
        return Err(unreachable_response());
    }
    Ok(deltas)
}
