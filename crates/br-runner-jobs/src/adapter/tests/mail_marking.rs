use std::num::NonZeroUsize;

use br_agentic_contract::{IncomingBatch, IncomingPort};
use br_agentic_harness_contract::MailPort;
use br_llm_messages::{AgentId, EntryId};
use br_runner_jobs_client::WriteKind;
use br_runner_jobs_client::test_double::{Call, ChildCommand};

use super::support::id;
use super::{World, cc_item, drained, mark_reads, notice_item, only_unread};
use crate::adapter::mark_gate_stored;

fn entry(n: u128) -> EntryId {
    EntryId::from_uuid(id(n)).expect("entry")
}

async fn list(world: &World, agent: AgentId) -> IncomingBatch {
    let context = world.context(agent);
    world
        .mail_port()
        .items(&context, None, NonZeroUsize::new(20).expect("limit"))
        .await
        .expect("a batch")
}

#[tokio::test]
async fn given_a_listing_through_the_mail_port_when_it_is_compared_with_the_incoming_port_then_the_items_the_batches_and_the_cursor_are_the_same()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    let items = (1..=3).map(|n| cc_item(agent.as_uuid(), n)).collect();
    world.jobs.graphql_default(only_unread(items));
    let gated = world.incoming_on_the_same_book(mark_gate_stored());
    let context = world.context(agent);
    let limit = NonZeroUsize::new(2).expect("limit");
    let by_mail = world
        .mail_port()
        .items(&context, None, limit)
        .await
        .expect("a batch");
    let by_incoming = gated.items(&context, None, limit).await.expect("a batch");
    assert_eq!(by_mail, by_incoming);
    let next = by_mail.next.as_ref();
    let more_by_mail = world
        .mail_port()
        .items(&context, next, limit)
        .await
        .expect("a batch");
    let more_by_incoming = gated.items(&context, next, limit).await.expect("a batch");
    assert_eq!(more_by_mail, more_by_incoming);
    assert_eq!(more_by_mail.items.len(), 1);
}

#[tokio::test]
async fn given_an_agent_that_holds_no_inbox_when_the_mail_port_lists_or_marks_then_each_call_is_an_error_of_the_port_and_jobs_is_not_read()
 {
    let world = World::mail().await;
    world.registered_agent().await;
    let stranger = AgentId::from_uuid(id(77)).expect("agent");
    let context = world.context(stranger);
    let error = world
        .mail_port()
        .items(&context, None, NonZeroUsize::new(2).expect("limit"))
        .await
        .expect_err("an error");
    assert!(!error.is_refusal());
    assert!(
        world
            .mail_port()
            .mark_read(&context, entry(201))
            .await
            .is_err()
    );
    assert!(
        world
            .mail_port()
            .documents(
                &context,
                br_agentic_contract::MailId::from_uuid(id(201)).expect("mail")
            )
            .await
            .is_err()
    );
    assert!(
        world
            .mail_port()
            .send(&context, super::mail_sending::new_mail(1))
            .await
            .is_err()
    );
    assert_eq!(
        super::count(&world.jobs, contract_jobs::graphql_roots::UNREAD_ROOT),
        0
    );
}

#[tokio::test]
async fn given_a_listed_mail_when_the_mail_port_marks_it_while_jobs_is_paused_then_the_call_returns_at_once_with_no_gate_and_the_item_leaves_the_listing()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(vec![
        cc_item(agent.as_uuid(), 1),
        cc_item(agent.as_uuid(), 2),
    ]));
    assert_eq!(list(&world, agent).await.items.len(), 2);
    world.jobs.pause();
    let context = world.context(agent);
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        world.mail_port().mark_read(&context, entry(201)),
    )
    .await
    .expect("the mark returns at once")
    .expect("marked");
    world.jobs.resume();
    assert_eq!(list(&world, agent).await.items.len(), 1);
    drained(&world.queue).await;
    assert_eq!(mark_reads(&world.jobs), vec![vec![id(101)]]);
}

#[tokio::test]
async fn given_a_run_completed_notice_when_the_mail_port_marks_it_then_the_queue_holds_the_finish_then_the_marker_when_the_call_returns_and_jobs_gets_them_in_that_order()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(vec![notice_item(
        agent.as_uuid(),
        1,
        "RUN_COMPLETED",
    )]));
    list(&world, agent).await;
    world.jobs.pause();
    let context = world.context(agent);
    world
        .mail_port()
        .mark_read(&context, entry(101))
        .await
        .expect("marked");
    let waiting: Vec<WriteKind> = world.queue.state().waiting.iter().map(|w| w.kind).collect();
    assert_eq!(waiting, vec![WriteKind::Finish, WriteKind::ReadMarker]);
    world.jobs.resume();
    drained(&world.queue).await;
    let calls = world.jobs.calls();
    let finish_at = calls
        .iter()
        .position(|c| matches!(c, Call::Command(ChildCommand::Finish { command_id, .. }) if *command_id == id(101)));
    let marker_at = calls
        .iter()
        .position(|c| matches!(c, Call::MarkRead { .. }));
    assert!(finish_at.is_some() && finish_at < marker_at);
}

#[tokio::test]
async fn given_a_mail_listed_by_one_port_when_the_other_marks_it_then_the_marker_reaches_jobs_once_and_a_second_mark_changes_nothing()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    let gated = world.incoming_on_the_same_book(mark_gate_stored());
    let context = world.context(agent);
    gated
        .items(&context, None, NonZeroUsize::new(5).expect("limit"))
        .await
        .expect("a batch");
    world
        .mail_port()
        .mark_read(&context, entry(201))
        .await
        .expect("marked");
    world
        .mail_port()
        .mark_read(&context, entry(201))
        .await
        .expect("again");
    gated
        .mark_read(&context, entry(201))
        .await
        .expect("through the other");
    drained(&world.queue).await;
    assert_eq!(mark_reads(&world.jobs), vec![vec![id(101)]]);
    assert!(list(&world, agent).await.items.is_empty());
}

#[tokio::test]
async fn given_a_pair_the_book_does_not_hold_when_the_mail_port_marks_it_then_nothing_is_handed_over_and_there_is_no_error()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    let context = world.context(agent);
    world
        .mail_port()
        .mark_read(&context, entry(999))
        .await
        .expect("no error");
    drained(&world.queue).await;
    assert!(mark_reads(&world.jobs).is_empty());
}
