use br_core_kernel::ServiceAccountId;
use contract_threads::{
    AiDelivery, AiTurn, CompleteMessageV2, CreateThread, FailMessageV2, LibraryJson, MessageTurn,
    PlacedAnnotation as WirePlaced, StoreMessageV2, StreamSeal, ThreadsCommand,
};
use serde::Serialize;
use uuid::Uuid;

use crate::closing::{Closing, ClosureKind};
use crate::error::Error;
use crate::ids::require_v7;
use crate::message::{OpenStream, WholeMessage};
use crate::thread::NewThread;
use crate::transport::BusCommand;

fn command<C: ThreadsCommand + Serialize>(
    command_id: Uuid,
    actor: ServiceAccountId,
    payload: &C,
) -> Result<BusCommand, Error> {
    require_v7(command_id, "a command id")?;
    let payload = serde_json::to_value(payload).map_err(|e| Error::invalid(e.to_string()))?;
    Ok(BusCommand {
        command_id,
        aggregate: C::AGGREGATE,
        verb: C::VERB,
        version: C::VERSION,
        actor,
        payload,
    })
}

pub(crate) fn create_thread(
    actor: ServiceAccountId,
    thread: &NewThread,
) -> Result<BusCommand, Error> {
    require_v7(thread.thread.as_uuid(), "a thread id")?;
    let payload = CreateThread {
        thread_id: thread.thread.as_uuid(),
        owner_id: thread.owner_id,
        user_ids: thread.member_ids.clone(),
        title: thread.title.clone(),
        resources: thread.resources.clone(),
        parent_thread_id: thread.origin.map(|o| o.parent_thread.as_uuid()),
        origin_block_id: thread.origin.map(|o| o.origin_block_id),
        acting_user_id: thread.acting_user_id,
    };
    command(thread.command_id, actor, &payload)
}

pub(crate) fn whole_message(
    actor: ServiceAccountId,
    message: &WholeMessage,
) -> Result<BusCommand, Error> {
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
    let payload = store(
        message.thread.as_uuid(),
        message.turn.id().as_uuid(),
        message.parent_message_id,
        message.job_id,
        message.acting_user_id,
        message.impersonator_id,
        AiTurn {
            agent_id: message.agent_id.as_uuid(),
            author_name: message.author_name.clone(),
            delivery: AiDelivery::Completed { turn, annotations },
        },
    );
    command(message.command_id, actor, &payload)
}

pub(crate) fn open_stream(actor: ServiceAccountId, open: &OpenStream) -> Result<BusCommand, Error> {
    let payload = store(
        open.thread.as_uuid(),
        open.message_id.as_uuid(),
        open.parent_message_id,
        open.job_id,
        open.acting_user_id,
        None,
        AiTurn {
            agent_id: open.agent_id.as_uuid(),
            author_name: open.author_name.clone(),
            delivery: AiDelivery::Streaming,
        },
    );
    command(open.command_id, actor, &payload)
}

fn store(
    thread_id: Uuid,
    message_id: Uuid,
    parent_message_id: Option<Uuid>,
    job_id: Option<Uuid>,
    acting_user_id: Option<Uuid>,
    impersonator_id: Option<Uuid>,
    turn: AiTurn,
) -> StoreMessageV2 {
    StoreMessageV2 {
        message_id,
        thread_id,
        parent_message_id,
        job_id,
        acting_user_id,
        impersonator_id,
        message: MessageTurn::Ai(turn),
    }
}

pub(crate) fn seal_of(chunks: &[contract_threads::Chunk]) -> Result<StreamSeal, Error> {
    let hash =
        contract_threads::seal_hash(chunks.iter()).map_err(|e| Error::invalid(e.to_string()))?;
    let last_seq = u64::try_from(chunks.len())
        .ok()
        .and_then(|count| count.checked_sub(1))
        .ok_or_else(|| Error::invalid("a seal needs at least one chunk"))?;
    Ok(StreamSeal { last_seq, hash })
}

pub(crate) fn closing(
    actor: ServiceAccountId,
    closing: &Closing,
    seal: Option<StreamSeal>,
) -> Result<BusCommand, Error> {
    let reason = closing.kind.reason(closing.detail.as_deref())?;
    let closure = Some(LibraryJson::of(&reason).map_err(|e| Error::invalid(e.to_string()))?);
    match (closing.kind.is_sealed(), seal) {
        (true, Some(seal)) => command(
            closing.command_id,
            actor,
            &CompleteMessageV2 {
                message_id: closing.message_id,
                thread_id: closing.thread.as_uuid(),
                seal,
                closure,
                acting_user_id: closing.acting_user_id,
            },
        ),
        _ => command(
            closing.command_id,
            actor,
            &FailMessageV2 {
                message_id: closing.message_id,
                thread_id: closing.thread.as_uuid(),
                seal: None,
                closure,
                acting_user_id: closing.acting_user_id,
            },
        ),
    }
}

pub(crate) fn needs_handle(kind: ClosureKind) -> bool {
    kind.is_sealed()
}
