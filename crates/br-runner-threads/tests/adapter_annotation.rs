//! An annotation through the adapter: an operation beyond the port that
//! queues an annotation chunk of an open AI turn, after an entry the adapter
//! queued into it, numbered and sealed with the turn's other chunks.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use br_agentic_contract::{AiTurnRef, ThreadRef};
use br_llm_messages::{ClosureReason, EntryId, TurnClosure, TurnEntry};
use br_runner_threads::test_double::CommandBody;
use br_runner_threads::{Annotation, AnnotationBody, Error, MemoryQueueStore};
use contract_threads::{Chunk, StreamSeal, seal_hash};
use support::adapter_wire::*;
use support::adapter_x::*;
use support::client_x::not_v7;
use support::*;

fn artifact() -> Annotation {
    Annotation::new(v7(), AnnotationBody::Artifact { artifact_id: v7() })
}

async fn plain_rig() -> Rig {
    rig_with_store(Arc::new(MemoryQueueStore::new())).await
}

async fn turn_with(rig: &Rig, entry: TurnEntry) -> (ThreadRef, AiTurnRef, EntryId) {
    let (thread, turn) = (thread_ref(), turn_ref(rig.agent));
    let id = entry.id();
    rig.access()
        .add_entry(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    (thread, turn, id)
}

fn sent(rig: &Rig, turn: &AiTurnRef) -> Vec<(u64, Chunk)> {
    chunks_of(&rig.threads, turn.id.as_uuid())
}

#[tokio::test]
async fn an_annotation_follows_its_entry_as_the_next_numbered_chunk() {
    let rig = plain_rig().await;
    let (thread, turn, entry) = turn_with(&rig, notice("the script ran")).await;
    let annotation = artifact();

    rig.adapter
        .add_annotation(&thread, turn.id, entry, annotation.clone())
        .await
        .expect("accepted");

    rig.idle().await;
    let chunks = sent(&rig, &turn);
    assert_eq!(chunks.len(), 2);
    let (
        seq,
        Chunk::Annotation {
            after_entry_id,
            annotation: placed,
        },
    ) = &chunks[1]
    else {
        panic!("not an annotation chunk");
    };
    assert_eq!(*seq, 1);
    assert_eq!(*after_entry_id, entry.as_uuid());
    assert_eq!(placed.id, annotation.id);
    assert_eq!(
        placed.body.artifact(),
        Some(match annotation.body {
            AnnotationBody::Artifact { artifact_id } => artifact_id,
            _ => unreachable!(),
        })
    );
}

#[tokio::test]
async fn an_annotation_may_follow_a_tool_result() {
    let rig = plain_rig().await;
    let (thread, turn) = (thread_ref(), turn_ref(rig.agent));
    let results = tool_results(entry_id(), block_id(), "execute_python", "done");
    let entry = results.id();
    rig.access()
        .complete_result(&thread, turn.clone(), results)
        .await
        .expect("accepted");

    rig.adapter
        .add_annotation(&thread, turn.id, entry, artifact())
        .await
        .expect("accepted");

    rig.idle().await;
    assert!(matches!(
        sent(&rig, &turn).last(),
        Some((1, Chunk::Annotation { after_entry_id, .. })) if *after_entry_id == entry.as_uuid()
    ));
}

#[tokio::test]
async fn an_annotation_may_follow_a_response_completed_without_a_stream() {
    let rig = plain_rig().await;
    let (thread, turn, entry) = (thread_ref(), turn_ref(rig.agent), entry_id());
    rig.access()
        .complete_response(
            &thread,
            turn.clone(),
            response_text(entry, "the files are ready"),
        )
        .await
        .expect("accepted");

    rig.adapter
        .add_annotation(&thread, turn.id, entry, artifact())
        .await
        .expect("accepted");

    rig.idle().await;
    assert!(matches!(
        sent(&rig, &turn).last(),
        Some((_, Chunk::Annotation { after_entry_id, .. })) if *after_entry_id == entry.as_uuid()
    ));
}

#[tokio::test]
async fn the_seal_of_the_closing_covers_the_annotation() {
    let rig = plain_rig().await;
    let (thread, turn, entry) = turn_with(&rig, notice("the script ran")).await;
    rig.adapter
        .add_annotation(&thread, turn.id, entry, artifact())
        .await
        .expect("accepted");

    rig.access()
        .close_turn(
            &thread,
            turn.clone(),
            TurnClosure::new(entry_id(), ClosureReason::completed()),
        )
        .await
        .expect("accepted");

    rig.idle().await;
    let chunks: Vec<Chunk> = sent(&rig, &turn).into_iter().map(|(_, c)| c).collect();
    let expected = StreamSeal {
        last_seq: 1,
        hash: seal_hash(chunks.iter()).expect("hash"),
    };
    let Some(CommandBody::Complete(complete)) =
        rig.threads.commands().last().map(|c| c.body.clone())
    else {
        panic!("the last command is not a completion");
    };
    assert_eq!(complete.seal, expected);
}

#[tokio::test]
async fn an_annotation_on_a_turn_the_adapter_did_not_open_is_refused() {
    let rig = plain_rig().await;

    let result = rig
        .adapter
        .add_annotation(&thread_ref(), turn_id(), entry_id(), artifact())
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    rig.idle().await;
    assert!(rig.threads.chunks().is_empty());
}

#[tokio::test]
async fn an_annotation_after_the_closing_was_accepted_is_refused() {
    let rig = plain_rig().await;
    let (thread, turn, entry) = turn_with(&rig, notice("the script ran")).await;
    rig.access()
        .close_turn(
            &thread,
            turn.clone(),
            TurnClosure::new(entry_id(), ClosureReason::completed()),
        )
        .await
        .expect("accepted");

    let result = rig
        .adapter
        .add_annotation(&thread, turn.id, entry, artifact())
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    rig.idle().await;
    assert_eq!(sent(&rig, &turn).len(), 1);
}

#[tokio::test]
async fn an_annotation_after_an_entry_not_queued_into_the_turn_is_refused() {
    let rig = plain_rig().await;
    let (thread, turn, _) = turn_with(&rig, notice("the script ran")).await;

    let result = rig
        .adapter
        .add_annotation(&thread, turn.id, entry_id(), artifact())
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    rig.idle().await;
    assert_eq!(sent(&rig, &turn).len(), 1);
}

#[tokio::test]
async fn an_annotation_whose_id_is_not_a_uuidv7_is_refused() {
    let rig = plain_rig().await;
    let (thread, turn, entry) = turn_with(&rig, notice("the script ran")).await;
    let annotation = Annotation::new(not_v7(), AnnotationBody::Artifact { artifact_id: v7() });

    let result = rig
        .adapter
        .add_annotation(&thread, turn.id, entry, annotation)
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    rig.idle().await;
    assert_eq!(sent(&rig, &turn).len(), 1);
}

#[tokio::test]
async fn an_annotation_calls_no_write_context_function() {
    let agent = agent_id();
    let calls = Arc::new(AtomicUsize::new(0));
    let rig = rig_over(
        Arc::new(MemoryQueueStore::new()),
        counting_context(agent, Arc::clone(&calls)),
        agent,
    )
    .await;
    let (thread, turn, entry) = turn_with(&rig, notice("the script ran")).await;
    let before = calls.load(Ordering::SeqCst);

    rig.adapter
        .add_annotation(&thread, turn.id, entry, artifact())
        .await
        .expect("accepted");

    assert_eq!(calls.load(Ordering::SeqCst), before);
}
