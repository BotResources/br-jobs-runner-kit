mod support;

use br_llm_messages::{ClosureReason, TurnClosure, TurnEntry};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{Annotation, AnnotationBody, Error};
use contract_threads::{AnnotationKind, Chunk};
use support::client_x::*;
use support::*;

#[tokio::test]
async fn a_delta_goes_to_the_bus_as_a_delta_chunk_of_the_message_with_number_zero() {
    let threads = MemoryThreads::new();
    let (handle, open) = opened(&threads.client()).await;
    let delta = started(open.message_id, entry_id());

    handle.delta(&delta).await.expect("taken");

    let chunks = threads.chunks();
    assert_eq!(chunks.len(), 1);
    let (message, seq, chunk) = &chunks[0];
    assert_eq!(*message, open.message_id.as_uuid());
    assert_eq!(*seq, 0);
    let Chunk::Delta { delta: sent } = chunk else {
        panic!("not a delta chunk: {chunk:?}");
    };
    assert_eq!(sent.to_delta().expect("reads back"), delta);
}

#[tokio::test]
async fn an_entry_goes_to_the_bus_as_an_entry_chunk() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    let entry = notice("hello");

    handle.entry(&entry).await.expect("taken");

    let Chunk::Entry { entry: sent } = &chunk_list(&threads)[0] else {
        panic!("not an entry chunk");
    };
    assert_eq!(sent.to_entry().expect("reads back"), entry);
}

#[tokio::test]
async fn an_annotation_goes_to_the_bus_naming_the_entry_it_follows() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    let after = entry_id();
    let annotation = Annotation::new(
        v7(),
        AnnotationBody::Warning {
            code: "slow".to_owned(),
            text: None,
        },
    );

    handle.annotation(after, &annotation).await.expect("taken");

    let Chunk::Annotation {
        after_entry_id,
        annotation: sent,
    } = &chunk_list(&threads)[0]
    else {
        panic!("not an annotation chunk");
    };
    assert_eq!(*after_entry_id, after.as_uuid());
    assert_eq!(sent.id, annotation.id);
    assert_eq!(sent.kind(), AnnotationKind::Warning);
}

#[tokio::test]
async fn a_keepalive_goes_to_the_bus_as_a_content_free_chunk() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;

    handle.keepalive().await.expect("taken");

    assert_eq!(chunk_list(&threads), vec![Chunk::Keepalive]);
}

#[tokio::test]
async fn chunks_of_every_kind_are_numbered_in_order_with_no_gap() {
    let threads = MemoryThreads::new();
    let (handle, open) = opened(&threads.client()).await;

    handle.keepalive().await.expect("taken");
    handle.entry(&notice("a")).await.expect("taken");
    handle
        .delta(&started(open.message_id, entry_id()))
        .await
        .expect("taken");
    handle
        .annotation(
            entry_id(),
            &Annotation::new(v7(), AnnotationBody::Artifact { artifact_id: v7() }),
        )
        .await
        .expect("taken");
    handle.entry(&notice("b")).await.expect("taken");

    assert_eq!(chunk_seqs(&threads), contiguous(5));
}

#[tokio::test]
async fn the_handle_and_its_writers_share_one_numbering() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    let writer = handle.writer();
    let other = writer.clone();

    handle.entry(&notice("a")).await.expect("taken");
    writer.entry(&notice("b")).await.expect("taken");
    other.keepalive().await.expect("taken");
    handle.keepalive().await.expect("taken");
    writer
        .annotation(
            entry_id(),
            &Annotation::new(v7(), AnnotationBody::JobReference { job_id: v7() }),
        )
        .await
        .expect("taken");

    assert_eq!(chunk_seqs(&threads), contiguous(5));
}

#[tokio::test]
async fn chunks_sent_from_many_tasks_at_once_are_taken_one_at_a_time_in_number_order() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    let mut tasks = Vec::new();
    for index in 0..40 {
        let writer = handle.writer();
        tasks.push(tokio::spawn(async move {
            if index % 2 == 0 {
                writer.keepalive().await
            } else {
                writer.entry(&notice("parallel")).await
            }
        }));
    }

    for task in tasks {
        task.await.expect("joined").expect("taken");
    }

    assert_eq!(chunk_seqs(&threads), contiguous(40));
}

#[tokio::test]
async fn a_delta_of_another_ai_turn_is_refused_and_uses_no_number() {
    let threads = MemoryThreads::new();
    let (handle, open) = opened(&threads.client()).await;
    let foreign = started(turn_id(), entry_id());

    let refused = handle.delta(&foreign).await;
    handle
        .delta(&started(open.message_id, entry_id()))
        .await
        .expect("taken");

    assert!(
        matches!(refused, Err(Error::InvalidInput(_))),
        "{refused:?}"
    );
    assert_eq!(chunk_seqs(&threads), vec![0]);
}

#[tokio::test]
async fn an_entry_that_is_a_model_response_is_refused_and_uses_no_number() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    let entry = TurnEntry::from(response_text(entry_id(), "hi"));

    let refused = handle.entry(&entry).await;
    handle.entry(&notice("fine")).await.expect("taken");

    assert!(
        matches!(refused, Err(Error::InvalidInput(_))),
        "{refused:?}"
    );
    assert_eq!(chunk_seqs(&threads), vec![0]);
}

#[tokio::test]
async fn an_entry_that_is_a_closure_is_refused_and_uses_no_number() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    let entry = TurnEntry::from(TurnClosure::new(entry_id(), ClosureReason::completed()));

    let refused = handle.writer().entry(&entry).await;

    assert!(
        matches!(refused, Err(Error::InvalidInput(_))),
        "{refused:?}"
    );
    assert!(threads.chunks().is_empty());
}

#[tokio::test]
async fn an_annotation_id_that_is_not_a_uuidv7_is_refused_and_uses_no_number() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    let bad = Annotation::new(
        not_v7(),
        AnnotationBody::Error {
            code: "boom".to_owned(),
            text: None,
        },
    );

    let refused = handle.annotation(entry_id(), &bad).await;
    handle.keepalive().await.expect("taken");

    assert!(
        matches!(refused, Err(Error::InvalidInput(_))),
        "{refused:?}"
    );
    assert_eq!(chunk_seqs(&threads), vec![0]);
}

#[tokio::test]
async fn the_chunks_of_a_handle_name_the_message_it_opened() {
    let threads = MemoryThreads::new();
    let client = threads.client();
    let (first, first_open) = opened(&client).await;
    let (second, second_open) = opened(&client).await;

    first.keepalive().await.expect("taken");
    second.keepalive().await.expect("taken");
    second.keepalive().await.expect("taken");

    let chunks = threads.chunks();
    assert_eq!(chunks[0].0, first_open.message_id.as_uuid());
    assert_eq!(chunks[1].0, second_open.message_id.as_uuid());
    assert_eq!(
        chunks.iter().map(|c| c.1).collect::<Vec<_>>(),
        vec![0, 0, 1]
    );
}
