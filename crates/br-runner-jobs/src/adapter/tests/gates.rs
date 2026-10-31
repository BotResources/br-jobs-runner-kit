use br_agentic_contract::InboxPort;
use br_llm_messages::EntryId;

use super::{ManualGate, World, cc_item, drained, mark_reads, notice_item, only_unread, yields};
use crate::book::tests::id;
use crate::queue::{Gate, QueueReport, Removal, WriteKind};
use crate::test_double::{Call, ChildCommand};

fn finishes(world: &World) -> Vec<(uuid::Uuid, uuid::Uuid)> {
    world
        .jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Command(ChildCommand::Finish {
                job_id, command_id, ..
            }) => Some((job_id, command_id)),
            _ => None,
        })
        .collect()
}

async fn list_all(world: &World, agent: br_llm_messages::AgentId) {
    let context = world.context(agent);
    world
        .inbox()
        .items(
            &context,
            None,
            std::num::NonZeroUsize::new(20).expect("limit"),
        )
        .await
        .expect("a batch");
}

#[tokio::test]
async fn given_a_call_notice_run_completed_when_the_gate_ends_stored_then_the_finish_is_sent_before_the_marker_as_requester_with_the_item_id_as_command_id()
 {
    let gate = ManualGate::new();
    let world = World::new(gate.gate()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(vec![notice_item(
        agent.as_uuid(),
        1,
        "RUN_COMPLETED",
    )]));
    list_all(&world, agent).await;
    let context = world.context(agent);
    world
        .inbox()
        .mark_read(&context, EntryId::from_uuid(id(101)).expect("entry"))
        .await
        .expect("marked");
    yields().await;
    assert!(finishes(&world).is_empty() && mark_reads(&world.jobs).is_empty());
    gate.end(Gate::Stored);
    drained(&world.queue).await;
    assert_eq!(finishes(&world), vec![(id(301), id(101))]);
    assert_eq!(mark_reads(&world.jobs), vec![vec![id(101)]]);
    let calls = world.jobs.calls();
    let finish_at = calls
        .iter()
        .position(|c| matches!(c, Call::Command(ChildCommand::Finish { .. })));
    let marker_at = calls
        .iter()
        .position(|c| matches!(c, Call::MarkRead { .. }));
    assert!(finish_at < marker_at);
}

#[tokio::test]
async fn given_a_call_notice_run_completed_when_the_gate_ends_dropped_then_no_finish_and_no_marker_are_sent_and_the_item_stays_out_of_the_listing()
 {
    let gate = ManualGate::new();
    let world = World::new(gate.gate()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(vec![notice_item(
        agent.as_uuid(),
        1,
        "RUN_COMPLETED",
    )]));
    list_all(&world, agent).await;
    let context = world.context(agent);
    world
        .inbox()
        .mark_read(&context, EntryId::from_uuid(id(101)).expect("entry"))
        .await
        .expect("marked");
    gate.end(Gate::Dropped);
    drained(&world.queue).await;
    assert!(finishes(&world).is_empty());
    assert!(mark_reads(&world.jobs).is_empty());
    let batch = world
        .inbox()
        .items(
            &context,
            None,
            std::num::NonZeroUsize::new(20).expect("limit"),
        )
        .await
        .expect("a batch");
    assert!(batch.items.is_empty());
    let removed = world.reports.all().into_iter().any(|report| {
        matches!(&report, QueueReport::Removed { write, removal }
            if write.kind == WriteKind::ReadMarker && *removal == Removal::DependsOnRemoved)
    });
    assert!(removed);
}

#[tokio::test]
async fn given_a_gate_that_ends_later_then_the_marker_waits_outside_the_queue_and_later_writes_are_not_held()
 {
    let gate = ManualGate::new();
    let world = World::new(gate.gate()).await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    list_all(&world, agent).await;
    let context = world.context(agent);
    world
        .inbox()
        .mark_read(&context, EntryId::from_uuid(id(201)).expect("entry"))
        .await
        .expect("marked");
    let later = world
        .queue
        .step_started(0, "a step".to_owned(), chrono::Utc::now());
    tokio::time::timeout(std::time::Duration::from_secs(5), later)
        .await
        .expect("the later write is delivered")
        .expect("delivered");
    let waiting = world.queue.state().waiting;
    assert!(
        waiting
            .iter()
            .any(|name| name.kind == WriteKind::ReadMarker)
    );
    assert!(mark_reads(&world.jobs).is_empty());
    gate.end(Gate::Stored);
    drained(&world.queue).await;
    assert_eq!(mark_reads(&world.jobs), vec![vec![id(101)]]);
}

#[tokio::test]
async fn given_a_gate_that_never_ends_then_the_drained_call_stays_pending_and_the_drop_of_the_queue_reports_the_marker_unsent_with_no_finish()
 {
    let gate = ManualGate::new();
    let world = World::new(gate.gate()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(vec![notice_item(
        agent.as_uuid(),
        1,
        "RUN_COMPLETED",
    )]));
    list_all(&world, agent).await;
    let context = world.context(agent);
    world
        .inbox()
        .mark_read(&context, EntryId::from_uuid(id(101)).expect("entry"))
        .await
        .expect("marked");
    yields().await;
    {
        use futures::FutureExt;
        assert!(world.queue.drained().now_or_never().is_none());
    }
    let reports = std::sync::Arc::clone(&world.reports);
    let jobs = std::sync::Arc::clone(&world.jobs);
    drop(world.queue);
    yields().await;
    let unsent = reports.all().into_iter().any(|report| {
        matches!(&report, QueueReport::Removed { write, removal }
            if write.kind == WriteKind::ReadMarker && *removal == Removal::LeftUnsent)
    });
    assert!(unsent);
    assert!(mark_reads(&jobs).is_empty());
    assert!(
        !jobs
            .calls()
            .iter()
            .any(|c| matches!(c, Call::Command(ChildCommand::Finish { .. })))
    );
}
