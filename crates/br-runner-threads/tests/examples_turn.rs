mod support;

#[path = "../examples/agent_turn_through_adapter.rs"]
#[allow(dead_code)] // the example is included for its functions; its main is unused here
mod agent_turn_through_adapter;

use agent_turn_through_adapter::{TurnEntries, discard_then_complete_then_close};
use br_llm_messages::{ContentBlock, DeltaOp, ResponseDelta};
use br_runner_threads::test_double::CommandBody;
use contract_threads::{Chunk, seal_hash};
use support::adapter_wire::{ai_of, chunks_of, deltas_of, folded, is_streaming, stores};
use support::queue_kit::within;
use support::*;

struct Played {
    world: World,
    turn: br_agentic_contract::AiTurnRef,
    entries: TurnEntries,
}

async fn played() -> Played {
    let world = world().await;
    let turn = turn_ref(world.agent);
    let entries = within(
        "the turn",
        discard_then_complete_then_close(world.access(), &thread_ref(), &turn),
    )
    .await
    .expect("the operations are accepted");
    eventually("the closing reaches Threads", || {
        world
            .threads
            .commands()
            .iter()
            .any(|command| matches!(command.body, CommandBody::Complete(_)))
    })
    .await;
    Played {
        world,
        turn,
        entries,
    }
}

fn of_entry(deltas: &[ResponseDelta], entry: br_llm_messages::EntryId) -> Vec<ResponseDelta> {
    deltas
        .iter()
        .filter(|delta| delta.entry() == entry)
        .cloned()
        .collect()
}

#[tokio::test]
async fn the_whole_turn_is_one_streaming_message_with_the_id_and_the_agent_of_the_turn() {
    let played = played().await;

    let stored = stores(&played.world.threads);

    assert_eq!(stored.len(), 1);
    assert!(is_streaming(&stored[0]));
    assert_eq!(stored[0].message_id, played.turn.id.as_uuid());
    assert_eq!(ai_of(&stored[0]).agent_id, played.turn.agent.as_uuid());
    assert_eq!(ai_of(&stored[0]).author_name, "Analyst");
}

#[tokio::test]
async fn the_chunks_of_the_turn_are_numbered_in_order_with_no_gap() {
    let played = played().await;

    let chunks = chunks_of(&played.world.threads, played.turn.id.as_uuid());

    let numbers: Vec<u64> = chunks.iter().map(|(seq, _)| *seq).collect();
    assert_eq!(numbers, (0..chunks.len() as u64).collect::<Vec<_>>());
}

#[tokio::test]
async fn the_discarded_response_ends_as_interrupted_and_never_as_completed() {
    let played = played().await;
    let deltas = deltas_of(&played.world.threads, played.turn.id.as_uuid());

    let discarded = of_entry(&deltas, played.entries.discarded);

    assert!(discarded.len() > 1);
    assert!(matches!(
        discarded.last().expect("a delta").op(),
        DeltaOp::ResponseInterrupted { .. }
    ));
    assert!(
        !discarded
            .iter()
            .any(|delta| matches!(delta.op(), DeltaOp::ResponseCompleted { .. }))
    );
}

#[tokio::test]
async fn the_completed_response_ends_with_the_completing_delta_and_folds_into_the_whole_response() {
    let played = played().await;
    let deltas = deltas_of(&played.world.threads, played.turn.id.as_uuid());

    let completed = of_entry(&deltas, played.entries.completed);

    assert!(matches!(
        completed.last().expect("a delta").op(),
        DeltaOp::ResponseCompleted { .. }
    ));
    let response = folded(&completed, played.turn.id, played.entries.completed)
        .expect("the deltas fold into a whole response");
    assert!(matches!(response.blocks(), [ContentBlock::Text(_)]));
}

#[tokio::test]
async fn the_discarded_response_comes_before_the_completed_response_in_the_chunks() {
    let played = played().await;
    let deltas = deltas_of(&played.world.threads, played.turn.id.as_uuid());

    let entries_in_order: Vec<_> = deltas.iter().map(|delta| delta.entry()).collect();

    let first_completed = entries_in_order
        .iter()
        .position(|entry| *entry == played.entries.completed)
        .expect("the completed response is in the chunks");
    assert!(
        entries_in_order[..first_completed]
            .iter()
            .all(|entry| *entry == played.entries.discarded)
    );
    assert!(first_completed > 0);
}

#[tokio::test]
async fn the_closing_carries_the_seal_of_every_chunk_of_the_turn() {
    let played = played().await;
    let threads = &played.world.threads;

    let complete = threads
        .commands()
        .into_iter()
        .find_map(|command| match command.body {
            CommandBody::Complete(complete) => Some(complete),
            _ => None,
        })
        .expect("a completion");

    let chunks: Vec<Chunk> = chunks_of(threads, played.turn.id.as_uuid())
        .into_iter()
        .map(|(_, chunk)| chunk)
        .collect();
    assert_eq!(complete.message_id, played.turn.id.as_uuid());
    assert_eq!(complete.seal.last_seq, chunks.len() as u64 - 1);
    assert_eq!(complete.seal.hash, seal_hash(&chunks).expect("a hash"));
}
