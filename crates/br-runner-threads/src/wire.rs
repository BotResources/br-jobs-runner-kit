use br_llm_messages::ClosureReason;
use contract_threads::{
    AiDelivery, AiTurn, Chunk, CompleteMessageV2, CreateThread, FailMessageV2, LibraryJson,
    MessageTurn, PlacedAnnotation as WirePlaced, StoreMessageV2, StreamSeal,
};
use uuid::Uuid;

use crate::backend::{Command, CommandBody};
use crate::closing::ClosureKind;
use crate::error::Error;
use crate::ids::require_v7;
use crate::message::{OpenStream, WholeMessage};
use crate::thread::NewThread;

pub(crate) fn create_thread(thread: &NewThread) -> Result<Command, Error> {
    require_v7(thread.command_id, "a command id")?;
    require_v7(thread.thread_id, "a thread id")?;
    Ok(Command {
        id: thread.command_id,
        body: CommandBody::CreateThread(CreateThread {
            thread_id: thread.thread_id,
            owner_id: thread.owner_id,
            user_ids: thread.member_ids.clone(),
            title: thread.title.clone(),
            resources: thread.resources.clone(),
            parent_thread_id: thread.origin.map(|o| o.parent_thread_id),
            origin_block_id: thread.origin.map(|o| o.origin_block_id),
            acting_user_id: thread.acting_user_id,
        }),
    })
}

pub(crate) fn whole_message(message: &WholeMessage) -> Result<Command, Error> {
    require_v7(message.command_id, "a command id")?;
    let annotations = message
        .annotations
        .iter()
        .map(|placed| {
            Ok(WirePlaced::new(
                placed.after_entry.as_uuid(),
                placed.annotation.to_wire()?,
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let turn = LibraryJson::of(&message.turn).map_err(|e| Error::invalid(e.to_string()))?;
    Ok(store(
        message.command_id,
        StoreParts {
            thread_id: message.thread_id,
            message_id: message.turn.id().as_uuid(),
            parent_message_id: message.parent_message_id,
            job_id: message.job_id,
            acting_user_id: message.acting_user_id,
            impersonator_id: message.impersonator_id,
        },
        AiTurn {
            agent_id: message.agent_id.as_uuid(),
            author_name: message.author_name.clone(),
            delivery: AiDelivery::Completed { turn, annotations },
        },
    ))
}

pub(crate) fn open_message(open: &OpenStream) -> Result<Command, Error> {
    require_v7(open.command_id, "a command id")?;
    Ok(store(
        open.command_id,
        StoreParts {
            thread_id: open.thread_id,
            message_id: open.message_id.as_uuid(),
            parent_message_id: open.parent_message_id,
            job_id: open.job_id,
            acting_user_id: open.acting_user_id,
            impersonator_id: open.impersonator_id,
        },
        AiTurn {
            agent_id: open.agent_id.as_uuid(),
            author_name: open.author_name.clone(),
            delivery: AiDelivery::Streaming,
        },
    ))
}

pub(crate) struct StoreParts {
    pub(crate) thread_id: Uuid,
    pub(crate) message_id: Uuid,
    pub(crate) parent_message_id: Option<Uuid>,
    pub(crate) job_id: Option<Uuid>,
    pub(crate) acting_user_id: Option<Uuid>,
    pub(crate) impersonator_id: Option<Uuid>,
}

pub(crate) fn store(command_id: Uuid, parts: StoreParts, turn: AiTurn) -> Command {
    Command {
        id: command_id,
        body: CommandBody::Store(StoreMessageV2 {
            message_id: parts.message_id,
            thread_id: parts.thread_id,
            parent_message_id: parts.parent_message_id,
            job_id: parts.job_id,
            acting_user_id: parts.acting_user_id,
            impersonator_id: parts.impersonator_id,
            message: MessageTurn::Ai(turn),
        }),
    }
}

pub(crate) struct CloseParts {
    pub(crate) command_id: Uuid,
    pub(crate) thread_id: Uuid,
    pub(crate) message_id: Uuid,
    pub(crate) reason: ClosureReason,
    pub(crate) seal: Option<StreamSeal>,
    pub(crate) acting_user_id: Option<Uuid>,
}

pub(crate) fn closing(parts: CloseParts) -> Result<Command, Error> {
    require_v7(parts.command_id, "a command id")?;
    let CloseParts {
        command_id,
        thread_id,
        message_id,
        reason,
        seal,
        acting_user_id,
    } = parts;
    let kind = ClosureKind::of(&reason)?;
    let closure = Some(LibraryJson::of(&reason).map_err(|e| Error::invalid(e.to_string()))?);
    let body = match kind {
        ClosureKind::Completed => CommandBody::Complete(CompleteMessageV2 {
            message_id,
            thread_id,
            seal: seal.ok_or_else(|| Error::invalid("a Completed closing needs a seal"))?,
            closure,
            acting_user_id,
        }),
        ClosureKind::BudgetExhausted
        | ClosureKind::Failed
        | ClosureKind::Interrupted
        | ClosureKind::Cancelled => CommandBody::Fail(FailMessageV2 {
            message_id,
            thread_id,
            seal,
            closure,
            acting_user_id,
        }),
    };
    Ok(Command {
        id: command_id,
        body,
    })
}

pub(crate) fn carries_content(chunks: &[Chunk]) -> bool {
    chunks
        .iter()
        .any(|chunk| !matches!(chunk, Chunk::Keepalive))
}

pub(crate) fn seal_of(chunks: &[Chunk]) -> Result<StreamSeal, Error> {
    let hash =
        contract_threads::seal_hash(chunks.iter()).map_err(|e| Error::invalid(e.to_string()))?;
    let last_seq = u64::try_from(chunks.len())
        .ok()
        .and_then(|count| count.checked_sub(1))
        .ok_or_else(|| Error::invalid("a seal needs at least one chunk"))?;
    Ok(StreamSeal { last_seq, hash })
}
