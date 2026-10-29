#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::time::Duration;

use br_agentic_contract::{InboxEntry, Incoming, Opened, ThreadAccess, ThreadError};
use br_graph_executor::{Observation, ObservedEvent};
use br_llm_messages::{
    AITurn, BlockId, ClosureReason, ContentBlock, IncomingEntry, ModelResponse, OpaqueJson,
    PlainBlock, ResponseEnding, ResponseMetadata, RuntimeNotice, RuntimeToolCall,
    RuntimeToolResult, TextBlock, ToolName, TurnClosure, TurnEntry,
};
use br_runner_threads::MessageStatus;
use br_runner_threads::transport::{Answer, Confirmed, Reply};
use support::*;

async fn open_turn(rig: &Rig, agent: br_llm_messages::AgentId) -> br_llm_messages::TurnId {
    rig.show(&rig.empty_thread(), agent);
    let turn = turn_id();
    let entry = InboxEntry::new(IncomingEntry::from(
        RuntimeNotice::new(entry_id(), "go").expect("a notice"),
    ))
    .expect("an inbox entry");
    rig.access
        .add_incoming(&rig.thread, agent, ai_target(turn), Incoming::Entry(entry))
        .await
        .expect("added");
    turn
}

fn conversation_with_pending_call(
    rig: &Rig,
    agent: br_llm_messages::AgentId,
    turn: br_llm_messages::TurnId,
) -> (br_llm_messages::Conversation, BlockId) {
    let call = BlockId::from_uuid(v7()).expect("a block id");
    let response = ModelResponse::new(
        entry_id(),
        vec![ContentBlock::from(
            RuntimeToolCall::new(
                call,
                ToolName::new("search").expect("a tool name"),
                OpaqueJson::parse("{}").expect("json"),
            )
            .expect("a call"),
        )],
        ResponseEnding::tool_use(),
        ResponseMetadata::new(),
    )
    .expect("a response");
    let mut ai = AITurn::open(turn, agent, author("Writer"));
    ai.push_entry(TurnEntry::from(response)).expect("pushed");
    let mut conversation = rig.empty_thread();
    conversation.push_turn(ai.into()).expect("pushed");
    (conversation, call)
}

fn result_for(call: BlockId) -> RuntimeToolResult {
    RuntimeToolResult::new(
        BlockId::from_uuid(v7()).expect("a block id"),
        call,
        ToolName::new("search").expect("a tool name"),
        false,
        vec![PlainBlock::Text(
            TextBlock::new(BlockId::from_uuid(v7()).expect("a block id"), "found").expect("text"),
        )],
    )
    .expect("a result")
}

#[tokio::test]
async fn given_chunks_sent_when_the_turn_is_closed_completed_then_the_closing_is_sealed() {
    let rig = rig();
    let agent = rig.live_agent().await;
    let turn = open_turn(&rig, agent).await;
    rig.show(&rig.empty_thread(), agent);
    let closure = TurnClosure::new(entry_id(), ClosureReason::completed());
    rig.access
        .close_turn(&rig.thread, agent, turn, closure)
        .await
        .expect("closed");
    assert_eq!(rig.payloads("complete").len(), 1);
    assert!(rig.payloads("fail").is_empty());
}

#[tokio::test]
async fn given_a_failed_closure_then_the_detail_travels_and_the_fail_command_is_used() {
    let rig = rig();
    let agent = rig.live_agent().await;
    let turn = open_turn(&rig, agent).await;
    rig.show(&rig.empty_thread(), agent);
    let closure = TurnClosure::new(
        entry_id(),
        ClosureReason::failed("the model broke").expect("a reason"),
    );
    rig.access
        .close_turn(&rig.thread, agent, turn, closure)
        .await
        .expect("closed");
    let fails = rig.payloads("fail");
    assert_eq!(fails.len(), 1);
    assert!(fails[0].to_string().contains("the model broke"));
}

#[tokio::test]
async fn given_threads_confirms_a_completed_closing_as_failed_then_it_is_a_port_error() {
    let rig = rig();
    let agent = rig.live_agent().await;
    let turn = open_turn(&rig, agent).await;
    rig.show(&rig.empty_thread(), agent);
    rig.bus.answer_with(|command| {
        Reply::Answer(Answer::Confirmed(Confirmed {
            command_id: command.command_id,
            status: Some(MessageStatus::Failed),
        }))
    });
    let closure = TurnClosure::new(entry_id(), ClosureReason::completed());
    let error = rig
        .access
        .close_turn(&rig.thread, agent, turn, closure)
        .await
        .expect_err("a port error");
    assert!(matches!(error, ThreadError::Fault(_)));
}

#[tokio::test(start_paused = true)]
async fn given_an_open_streaming_message_then_a_keepalive_goes_every_five_minutes_until_the_closing()
 {
    let rig = rig();
    let agent = rig.live_agent().await;
    let turn = open_turn(&rig, agent).await;
    assert_eq!(rig.chunk_kinds(), ["ENTRY"]);
    tokio::time::sleep(Duration::from_secs(301)).await;
    assert_eq!(rig.chunk_kinds(), ["ENTRY", "KEEPALIVE"]);
    tokio::time::sleep(Duration::from_secs(300)).await;
    assert_eq!(rig.chunk_kinds().len(), 3);
    rig.show(&rig.empty_thread(), agent);
    rig.access
        .close_turn(
            &rig.thread,
            agent,
            turn,
            TurnClosure::new(entry_id(), ClosureReason::cancelled()),
        )
        .await
        .expect("closed");
    tokio::time::sleep(Duration::from_secs(1200)).await;
    assert_eq!(rig.chunk_kinds().len(), 3);
}

#[tokio::test(start_paused = true)]
async fn given_the_instance_ended_then_calls_fail_nothing_is_sent_and_keepalives_stop() {
    let rig = rig();
    let instance = rig.create("writer");
    let agent = rig.agent(instance, "main").await.id;
    let turn = open_turn(&rig, agent).await;
    let (response, block) = text_response("Hello");
    rig.show(&rig.empty_thread(), agent);
    let Opened::Stream(mut sink) = rig
        .access
        .open_response(&rig.thread, agent, ai_target(turn), response.id())
        .await
        .expect("opened")
    else {
        panic!("a stream is expected");
    };
    rig.handle.observer().observe(Observation::EventHandled {
        node: "writer".to_owned(),
        instance,
        event: ObservedEvent::Result,
    });
    let before = rig.bus.chunks().len();
    sink.send(br_llm_messages::ResponseDelta::new(
        turn,
        response.id(),
        br_llm_messages::DeltaOp::text_appended(block, "x"),
    ))
    .await;
    tokio::time::sleep(Duration::from_secs(900)).await;
    assert_eq!(rig.bus.chunks().len(), before);
    rig.show(&rig.empty_thread(), agent);
    let error = rig
        .access
        .load(&rig.thread, agent)
        .await
        .expect_err("no live instance");
    let port = error
        .cause()
        .downcast_ref::<br_runner_toolkit::PortError>()
        .expect("a port error");
    assert_eq!(
        port.code(),
        br_runner_toolkit::PortErrorCode::NoLiveInstance
    );
}

#[tokio::test]
async fn given_a_tool_call_not_pending_when_a_result_is_opened_then_it_is_refused() {
    let rig = rig();
    let agent = rig.live_agent().await;
    let turn = turn_id();
    let (conversation, _) = conversation_with_pending_call(&rig, agent, turn);
    rig.show(&conversation, agent);
    let other_call = BlockId::from_uuid(v7()).expect("a block id");
    let refused = rig
        .access
        .open_result(&rig.thread, agent, ai_target(turn), entry_id(), other_call)
        .await;
    assert!(matches!(refused, Err(ThreadError::Refused(_))));
}

#[tokio::test]
async fn given_a_pending_call_when_a_result_is_opened_then_fragments_go_nowhere_and_the_whole_result_is_one_entry_chunk()
 {
    let rig = rig();
    let agent = rig.live_agent().await;
    let turn = open_turn(&rig, agent).await;
    let (conversation, call) = conversation_with_pending_call(&rig, agent, turn);
    rig.show_times(&conversation, agent, 2);
    let entry = entry_id();
    let Opened::Stream(mut stream) = rig
        .access
        .open_result(&rig.thread, agent, ai_target(turn), entry, call)
        .await
        .expect("opened")
    else {
        panic!("a stream is expected");
    };
    let before = rig.bus.chunks().len();
    stream.send("partial".to_owned()).await;
    assert_eq!(rig.bus.chunks().len(), before);
    rig.access
        .complete_result(&rig.thread, agent, ai_target(turn), entry, result_for(call))
        .await
        .expect("completed");
    assert_eq!(rig.bus.chunks().len(), before + 1);
    assert_eq!(rig.chunk_kinds().last().map(String::as_str), Some("ENTRY"));
}

#[tokio::test]
async fn given_the_run_aborted_then_no_closure_keepalive_or_chunk_follows() {
    let rig = rig();
    let agent = rig.live_agent().await;
    let turn = open_turn(&rig, agent).await;
    let commands = rig.bus.commands().len();
    let chunks = rig.bus.chunks().len();
    rig.handle.end();
    rig.show(&rig.empty_thread(), agent);
    let error = rig
        .access
        .close_turn(
            &rig.thread,
            agent,
            turn,
            TurnClosure::new(entry_id(), ClosureReason::completed()),
        )
        .await
        .expect_err("run ended");
    assert!(matches!(error, ThreadError::Fault(_)));
    assert_eq!(rig.bus.commands().len(), commands);
    assert_eq!(rig.bus.chunks().len(), chunks);
}
