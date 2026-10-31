use std::sync::Arc;

use br_agentic_contract::{AiTurnRef, PortError, ThreadRef};
use contract_threads::{AiDelivery, AiTurn, Chunk};
use uuid::Uuid;

use crate::adapter::inner::{Inner, Mem, Slot, TurnState};
use crate::adapter::settings::WriteContext;
use crate::backend::{Command, CommandBody};
use crate::error::Error;
use crate::queue::item::{Item, ItemKind, Payload};
use crate::wire::{self, StoreParts};

pub(crate) fn fault(error: Error) -> PortError {
    PortError::fault(error)
}

pub(crate) fn closed_turn() -> Error {
    Error::invalid("the closing of this turn was already accepted")
}

fn command_item(
    slot: &Slot,
    command: Command,
    agent: Uuid,
    message: Uuid,
    entries: Vec<Uuid>,
    kind: ItemKind,
) -> Item {
    Item {
        id: command.id,
        thread: slot.thread.as_uuid(),
        agent,
        message,
        entries,
        kind,
        payload: Payload::Command(command.body),
    }
}

impl Inner {
    pub(crate) async fn context_for(&self, thread: &ThreadRef) -> Result<WriteContext, PortError> {
        self.write_context.call(*thread).await
    }

    pub(crate) async fn ensure_turn(
        self: &Arc<Self>,
        slot: &Arc<Slot>,
        mem: &mut Mem,
        turn: &AiTurnRef,
        ctx: &WriteContext,
    ) -> Result<(), Error> {
        let message = turn.id.as_uuid();
        if let Some(state) = mem.turns.get(&message) {
            return if state.closed {
                Err(closed_turn())
            } else {
                Ok(())
            };
        }
        let command = wire::store(
            Uuid::now_v7(),
            StoreParts {
                thread_id: slot.thread.as_uuid(),
                message_id: message,
                parent_message_id: None,
                job_id: ctx.job_id,
                acting_user_id: ctx.acting_user_id,
                impersonator_id: ctx.impersonator_id,
            },
            AiTurn {
                agent_id: turn.agent.as_uuid(),
                author_name: turn.author.as_str().to_owned(),
                delivery: AiDelivery::Streaming,
            },
        );
        let item = command_item(
            slot,
            command,
            turn.agent.as_uuid(),
            message,
            Vec::new(),
            ItemKind::Opening,
        );
        self.accept(slot, item).await?;
        mem.turns.insert(
            message,
            TurnState {
                agent: turn.agent,
                chunks: Vec::new(),
                closed: false,
            },
        );
        Ok(())
    }

    pub(crate) async fn push_chunk(
        self: &Arc<Self>,
        slot: &Arc<Slot>,
        mem: &mut Mem,
        message: Uuid,
        entries: Vec<Uuid>,
        chunk: Chunk,
    ) -> Result<(), Error> {
        let state = mem.turns.get_mut(&message).ok_or(Error::StreamClosed)?;
        if state.closed {
            return Err(Error::StreamClosed);
        }
        let seq =
            u64::try_from(state.chunks.len()).map_err(|_| Error::invalid("too many chunks"))?;
        let item = Item {
            id: Uuid::now_v7(),
            thread: slot.thread.as_uuid(),
            agent: state.agent.as_uuid(),
            message,
            entries,
            kind: ItemKind::Chunk,
            payload: Payload::Chunk {
                seq,
                chunk: chunk.clone(),
            },
        };
        self.accept(slot, item).await?;
        state.chunks.push(chunk);
        Ok(())
    }

    pub(crate) fn closing_item(slot: &Slot, command: Command, agent: Uuid, message: Uuid) -> Item {
        command_item(slot, command, agent, message, Vec::new(), ItemKind::Closing)
    }

    pub(crate) fn whole_item(
        slot: &Slot,
        command: Command,
        agent: Uuid,
        message: Uuid,
        entries: Vec<Uuid>,
    ) -> Item {
        debug_assert!(matches!(command.body, CommandBody::Store(_)));
        command_item(
            slot,
            command,
            agent,
            message,
            entries,
            ItemKind::WholeMessage,
        )
    }
}
