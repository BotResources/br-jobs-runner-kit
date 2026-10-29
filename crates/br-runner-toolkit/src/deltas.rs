use br_llm_messages::{
    ContentBlock, DeltaOp, DraftPhase, Finalization, ModelResponse, ResponseDelta, ResponseDraft,
    SlotLifecycle,
};

pub(crate) fn remaining(draft: &ResponseDraft, response: &ModelResponse) -> Vec<ResponseDelta> {
    let turn = draft.turn();
    let entry = draft.entry();
    let make = |op: DeltaOp| ResponseDelta::new(turn, entry, op);
    let mut deltas = Vec::new();
    let view = draft.view();
    if matches!(draft.phase(), DraftPhase::Unstarted { .. }) {
        deltas.push(make(DeltaOp::response_started(response.metadata())));
    }
    for block in response.blocks() {
        match view.slot(block.id()) {
            Some(slot) if slot.lifecycle() == SlotLifecycle::Open => {
                deltas.push(make(DeltaOp::block_finalized(
                    block.id(),
                    Finalization::authoritative(ContentBlock::clone(block)),
                )));
            }
            Some(_) => {}
            None => deltas.push(make(DeltaOp::block_inserted(block.clone()))),
        }
    }
    if !matches!(draft.phase(), DraftPhase::Completed { .. }) {
        deltas.push(make(DeltaOp::response_completed(response.ending().clone())));
    }
    deltas
}
