use std::sync::Arc;

use br_agentic_contract::{AiTurnRef, PortError, ResponseStream, ThreadRef};
use br_llm_messages::{EntryId, ModelResponse};

use super::turn::{closed_turn, fault};
use crate::adapter::inner::{Inner, OpenResponse, Slot};
use crate::adapter::lane::QueueLane;
use crate::adapter::settings::WriteContext;
use crate::response::{ResponseControl, pair};

impl Inner {
    async fn needs_opening(&self, slot: &Slot, turn: &AiTurnRef) -> Result<bool, PortError> {
        match slot.mem.lock().await.turns.get(&turn.id.as_uuid()) {
            None => Ok(true),
            Some(state) if state.closed => Err(fault(closed_turn())),
            Some(_) => Ok(false),
        }
    }

    async fn opening_context(
        &self,
        thread: &ThreadRef,
        slot: &Slot,
        turn: &AiTurnRef,
    ) -> Result<Option<WriteContext>, PortError> {
        if self.needs_opening(slot, turn).await? {
            Ok(Some(self.context_for(thread).await?))
        } else {
            Ok(None)
        }
    }

    fn controls(
        self: &Arc<Self>,
        slot: &Arc<Slot>,
        turn: &AiTurnRef,
        entry: EntryId,
    ) -> (crate::response::ResponseSink, Arc<ResponseControl>) {
        let lane = QueueLane::new(Arc::clone(self), Arc::clone(slot), turn.id.as_uuid());
        let (sink, control) = pair(turn.id, entry, Arc::new(lane));
        (sink, Arc::new(control))
    }

    pub(crate) async fn open_response(
        self: &Arc<Self>,
        thread: &ThreadRef,
        turn: AiTurnRef,
        entry: EntryId,
    ) -> Result<ResponseStream, PortError> {
        let slot = self.slot(*thread);
        let ctx = self.opening_context(thread, &slot, &turn).await?;
        let earlier = {
            let mut mem = slot.mem.lock().await;
            if let Some(ctx) = &ctx {
                self.ensure_turn(&slot, &mut mem, &turn, ctx)
                    .await
                    .map_err(fault)?;
            }
            mem.responses
                .get(&entry)
                .map(|open| Arc::clone(&open.control))
        };
        if let Some(earlier) = earlier {
            earlier.discard().await.map_err(fault)?;
            earlier.end().await;
        }
        let (sink, control) = self.controls(&slot, &turn, entry);
        slot.mem.lock().await.responses.insert(
            entry,
            OpenResponse {
                agent: turn.agent,
                control,
            },
        );
        Ok(Box::new(sink))
    }

    pub(crate) async fn complete_response(
        self: &Arc<Self>,
        thread: &ThreadRef,
        turn: AiTurnRef,
        response: ModelResponse,
    ) -> Result<(), PortError> {
        let ctx = self.context_for(thread).await?;
        let slot = self.slot(*thread);
        let entry = response.id();
        let held = {
            let mut mem = slot.mem.lock().await;
            self.ensure_turn(&slot, &mut mem, &turn, &ctx)
                .await
                .map_err(fault)?;
            mem.responses
                .get(&entry)
                .map(|open| Arc::clone(&open.control))
        };
        let control = match held {
            Some(control) => control,
            None => self.controls(&slot, &turn, entry).1,
        };
        let outcome = control.complete(&response).await;
        if outcome.is_ok() {
            control.end().await;
            slot.mem.lock().await.responses.remove(&entry);
        }
        outcome.map_err(fault)
    }

    pub(crate) async fn discard_response(
        self: &Arc<Self>,
        thread: &ThreadRef,
        entry: EntryId,
    ) -> Result<(), PortError> {
        let slot = self.slot(*thread);
        let held = slot
            .mem
            .lock()
            .await
            .responses
            .get(&entry)
            .map(|open| Arc::clone(&open.control));
        let Some(control) = held else {
            return Ok(());
        };
        control.discard().await.map_err(fault)?;
        control.end().await;
        slot.mem.lock().await.responses.remove(&entry);
        Ok(())
    }

    pub(crate) async fn open_result(
        self: &Arc<Self>,
        thread: &ThreadRef,
        turn: AiTurnRef,
    ) -> Result<(), PortError> {
        let slot = self.slot(*thread);
        let Some(ctx) = self.opening_context(thread, &slot, &turn).await? else {
            return Ok(());
        };
        let mut mem = slot.mem.lock().await;
        self.ensure_turn(&slot, &mut mem, &turn, &ctx)
            .await
            .map_err(fault)
    }
}
