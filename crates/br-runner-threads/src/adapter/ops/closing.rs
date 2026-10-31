use std::sync::Arc;

use br_agentic_contract::{AiTurnRef, PortError, ThreadRef};
use br_llm_messages::TurnClosure;
use uuid::Uuid;

use super::turn::{closed_turn, fault};
use crate::adapter::inner::{Inner, TurnState};
use crate::closing::ClosureKind;
use crate::error::Error;
use crate::wire::{self, CloseParts};

impl Inner {
    pub(crate) async fn close_turn(
        self: &Arc<Self>,
        thread: &ThreadRef,
        turn: AiTurnRef,
        closure: TurnClosure,
    ) -> Result<(), PortError> {
        let kind = ClosureKind::of(closure.reason()).map_err(fault)?;
        let ctx = self.context_for(thread).await?;
        let slot = self.slot(*thread);
        let thread = *thread;
        self.ordered(&slot, move |inner, slot| async move {
            let message = turn.id.as_uuid();
            let mut mem = slot.mem.lock().await;
            let chunks = match mem.turns.get(&message) {
                Some(state) if state.closed => return Err(closed_turn()),
                Some(state) => state.chunks.as_slice(),
                None => &[],
            };
            let content = wire::carries_content(chunks);
            if kind == ClosureKind::Completed && !content {
                return Err(Error::invalid(
                    "a Completed closing for a turn that holds no state or no chunk",
                ));
            }
            let seal = if content {
                Some(wire::seal_of(chunks)?)
            } else {
                None
            };
            let command = wire::closing(CloseParts {
                command_id: Uuid::now_v7(),
                thread_id: thread.as_uuid(),
                message_id: message,
                reason: closure.reason().clone(),
                seal,
                acting_user_id: ctx.acting_user_id,
            })?;
            let item = Inner::closing_item(&slot, command, turn.agent.as_uuid(), message);
            inner.accept(&slot, item).await?;
            let state = mem
                .turns
                .entry(message)
                .or_insert_with(|| TurnState::closed_by(turn.agent));
            state.closed = true;
            state.keepalive = None;
            Ok(())
        })
        .await
        .map_err(fault)
    }
}
