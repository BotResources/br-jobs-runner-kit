#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use br_agentic_contract::{Completed, InboxEntry, Incoming, Opened, ThreadAccess, ThreadError};
use br_llm_messages::{
    AITurn, BlockStart, ClosureReason, DeltaOp, ModelResponse, ResponseDelta, ResponseDraft,
    RuntimeNotice, TurnClosure, TurnEntry,
};
use support::*;

fn replay(
    deltas: &[ResponseDelta],
    turn: br_llm_messages::TurnId,
    response: &ModelResponse,
) -> ModelResponse {
    let mut draft = ResponseDraft::new(turn, response.id());
    for delta in deltas {
        draft.apply(delta).expect("the delta fits");
    }
    draft.into_response().expect("a finished response")
}

#[tokio::test]
async fn given_a_response_completed_without_being_opened_then_the_chunks_rebuild_it_whole() {
    let rig = rig();
    let agent = rig.live_agent().await;
    rig.show(&rig.empty_thread(), agent);
    let turn = turn_id();
    let (response, _) = text_response("Hello");
    let done = rig
        .access
        .complete_response(&rig.thread, agent, ai_target(turn), response.clone())
        .await
        .expect("completed");
    assert_eq!(done, Completed::Added);
    assert_eq!(rig.payloads("store").len(), 1);
    assert_eq!(rig.payloads("store")[0]["message"]["status"], "STREAMING");
    assert_eq!(replay(&rig.sent_deltas(), turn, &response), response);
}

#[tokio::test]
async fn given_a_partly_streamed_response_when_completed_then_only_the_rest_is_sent() {
    let rig = rig();
    let agent = rig.live_agent().await;
    rig.show_times(&rig.empty_thread(), agent, 2);
    let turn = turn_id();
    let (response, block) = text_response("Hello");
    let Opened::Stream(mut sink) = rig
        .access
        .open_response(&rig.thread, agent, ai_target(turn), response.id())
        .await
        .expect("opened")
    else {
        panic!("a stream is expected");
    };
    let make = |op| ResponseDelta::new(turn, response.id(), op);
    sink.send(make(DeltaOp::response_started(response.metadata())))
        .await;
    sink.send(make(DeltaOp::block_started(block, BlockStart::text())))
        .await;
    sink.send(make(DeltaOp::text_appended(block, "Hel"))).await;
    assert_eq!(rig.sent_deltas().len(), 3);
    rig.access
        .complete_response(&rig.thread, agent, ai_target(turn), response.clone())
        .await
        .expect("completed");
    let sent = rig.sent_deltas();
    assert_eq!(sent.len(), 5);
    assert!(matches!(sent[3].op(), DeltaOp::BlockFinalized { .. }));
    assert!(matches!(sent[4].op(), DeltaOp::ResponseCompleted { .. }));
    assert_eq!(replay(&sent, turn, &response), response);
    let seqs: Vec<u64> = rig.bus.chunks().iter().map(|chunk| chunk.seq).collect();
    assert_eq!(seqs, (0..5).collect::<Vec<u64>>());
}

#[tokio::test]
async fn given_a_discarded_response_then_nothing_more_is_sent_and_the_discard_is_repeatable() {
    let rig = rig();
    let agent = rig.live_agent().await;
    rig.show(&rig.empty_thread(), agent);
    let turn = turn_id();
    let (response, _) = text_response("Hello");
    let Opened::Stream(mut sink) = rig
        .access
        .open_response(&rig.thread, agent, ai_target(turn), response.id())
        .await
        .expect("opened")
    else {
        panic!("a stream is expected");
    };
    sink.send(ResponseDelta::new(
        turn,
        response.id(),
        DeltaOp::response_started(response.metadata()),
    ))
    .await;
    rig.access
        .discard_response(&rig.thread, agent, response.id())
        .await
        .expect("discarded");
    rig.access
        .discard_response(&rig.thread, agent, response.id())
        .await
        .expect("discarded twice");
    rig.access
        .discard_response(&rig.thread, agent, entry_id())
        .await
        .expect("never opened");
    sink.send(ResponseDelta::new(
        turn,
        response.id(),
        DeltaOp::text_appended(response.blocks()[0].id(), "x"),
    ))
    .await;
    assert_eq!(rig.sent_deltas().len(), 1);
    assert!(rig.payloads("complete").is_empty() && rig.payloads("fail").is_empty());
}

#[tokio::test]
async fn given_a_response_the_thread_holds_when_opened_then_the_held_response_comes_back() {
    let rig = rig();
    let agent = rig.live_agent().await;
    let turn = turn_id();
    let (response, _) = text_response("Hello");
    let mut conversation = rig.empty_thread();
    let mut ai = AITurn::open(turn, agent, author("Writer"));
    ai.push_entry(TurnEntry::from(response.clone()))
        .expect("pushed");
    conversation.push_turn(ai.into()).expect("pushed");
    rig.show_times(&conversation, agent, 2);
    let opened = rig
        .access
        .open_response(&rig.thread, agent, ai_target(turn), response.id())
        .await
        .expect("held");
    assert!(matches!(opened, Opened::Held(held) if held == response));
    let completed = rig
        .access
        .complete_response(&rig.thread, agent, ai_target(turn), response.clone())
        .await
        .expect("held");
    assert_eq!(completed, Completed::Held(response));
    assert!(rig.bus.commands().is_empty() && rig.bus.chunks().is_empty());
}

#[tokio::test]
async fn given_a_closed_ai_turn_or_another_author_when_a_response_is_opened_then_it_is_refused() {
    let rig = rig();
    let agent = rig.live_agent().await;
    let turn = turn_id();
    let (first, _) = text_response("Hello");
    let mut conversation = rig.empty_thread();
    let mut ai = AITurn::open(turn, agent, author("Writer"));
    ai.push_entry(TurnEntry::from(first)).expect("pushed");
    ai.push_entry(TurnEntry::from(TurnClosure::new(
        entry_id(),
        ClosureReason::completed(),
    )))
    .expect("closed");
    conversation.push_turn(ai.into()).expect("pushed");
    rig.show_times(&conversation, agent, 2);
    let closed = rig
        .access
        .open_response(&rig.thread, agent, ai_target(turn), entry_id())
        .await;
    assert!(matches!(closed, Err(ThreadError::Refused(_))));
    let other = br_agentic_contract::TurnTarget::Ai {
        id: turn,
        author: author("Somebody else"),
    };
    let foreign = rig
        .access
        .open_response(&rig.thread, agent, other, entry_id())
        .await;
    assert!(matches!(foreign, Err(ThreadError::Refused(_))));
    assert!(rig.bus.commands().is_empty());
}

#[tokio::test]
async fn given_two_additions_to_one_ai_turn_then_one_streaming_message_is_opened() {
    let rig = rig();
    let agent = rig.live_agent().await;
    rig.show_times(&rig.empty_thread(), agent, 2);
    let turn = turn_id();
    for text in ["one", "two"] {
        let entry = InboxEntry::new(br_llm_messages::IncomingEntry::from(
            RuntimeNotice::new(entry_id(), text).expect("a notice"),
        ))
        .expect("an inbox entry");
        rig.access
            .add_incoming(&rig.thread, agent, ai_target(turn), Incoming::Entry(entry))
            .await
            .expect("added");
    }
    assert_eq!(rig.payloads("store").len(), 1);
    assert_eq!(rig.chunk_kinds(), ["ENTRY", "ENTRY"]);
}
