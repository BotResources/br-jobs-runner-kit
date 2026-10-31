use std::sync::Arc;

use br_agentic_contract::{AiTurnRef, PortError, ThreadRef};
use br_llm_messages::{InputTurn, RuntimeToolResults, Turn, TurnEntry, TurnEntryKind};
use contract_threads::{AiDelivery, AiTurn, LibraryJson};
use uuid::Uuid;

use super::turn::fault;
use crate::adapter::inner::Inner;
use crate::error::Error;
use crate::stream::writes::entry_chunk;
use crate::wire::{self, StoreParts};

impl Inner {
    pub(crate) async fn add_input_turn(
        self: &Arc<Self>,
        thread: &ThreadRef,
        turn: InputTurn,
    ) -> Result<(), PortError> {
        let ctx = self.context_for(thread).await?;
        let message = turn.id().as_uuid();
        let entries: Vec<Uuid> = turn.entries().iter().map(|e| e.id().as_uuid()).collect();
        let body = LibraryJson::of(&Turn::InputTurn(turn))
            .map_err(|e| fault(Error::invalid(e.to_string())))?;
        let command = wire::store(
            Uuid::now_v7(),
            StoreParts {
                thread_id: thread.as_uuid(),
                message_id: message,
                parent_message_id: None,
                job_id: ctx.job_id,
                acting_user_id: ctx.acting_user_id,
                impersonator_id: ctx.impersonator_id,
            },
            AiTurn {
                agent_id: ctx.agent.as_uuid(),
                author_name: ctx.author.as_str().to_owned(),
                delivery: AiDelivery::Completed {
                    turn: body,
                    annotations: Vec::new(),
                },
            },
        );
        let slot = self.slot(*thread);
        let _order = slot.order.lock().await;
        let _ordered = slot.mem.lock().await;
        let item = Inner::whole_item(&slot, command, ctx.agent.as_uuid(), message, entries);
        self.accept(&slot, item).await.map_err(fault)
    }

    pub(crate) async fn add_entry(
        self: &Arc<Self>,
        thread: &ThreadRef,
        turn: AiTurnRef,
        entry: TurnEntry,
    ) -> Result<(), PortError> {
        if matches!(
            entry.kind(),
            TurnEntryKind::ModelResponse | TurnEntryKind::TurnClosure
        ) {
            return Err(fault(Error::invalid(
                "an entry that is a model response or a closure",
            )));
        }
        let chunk = entry_chunk(&entry).map_err(fault)?;
        let ctx = self.context_for(thread).await?;
        self.entry_into_turn(thread, &turn, &ctx, entry.id().as_uuid(), chunk)
            .await
    }

    pub(crate) async fn complete_result(
        self: &Arc<Self>,
        thread: &ThreadRef,
        turn: AiTurnRef,
        results: RuntimeToolResults,
    ) -> Result<(), PortError> {
        let entry = TurnEntry::from(results);
        let chunk = entry_chunk(&entry).map_err(fault)?;
        let ctx = self.context_for(thread).await?;
        self.entry_into_turn(thread, &turn, &ctx, entry.id().as_uuid(), chunk)
            .await
    }

    async fn entry_into_turn(
        self: &Arc<Self>,
        thread: &ThreadRef,
        turn: &AiTurnRef,
        ctx: &crate::adapter::settings::WriteContext,
        entry: Uuid,
        chunk: contract_threads::Chunk,
    ) -> Result<(), PortError> {
        let slot = self.slot(*thread);
        let _order = slot.order.lock().await;
        let mut mem = slot.mem.lock().await;
        self.ensure_turn(&slot, &mut mem, turn, ctx)
            .await
            .map_err(fault)?;
        self.push_chunk(&slot, &mut mem, turn.id.as_uuid(), vec![entry], chunk)
            .await
            .map_err(fault)
    }
}
