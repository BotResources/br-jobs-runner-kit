use std::num::NonZeroUsize;
use std::time::Duration;

use br_agentic_contract::{Cursor, InboxBatch, InboxItem, InboxPort, MailItem};
use contract_jobs::graphql_roots::{MAIL_ROOT, UNREAD_ROOT};
use futures::FutureExt;

use super::{
    World, attachment_view, cc_item, count, drained, mail_item, mail_view, only_unread, wire,
    yields,
};
use crate::adapter::mark_gate_stored;
use crate::book::tests::{id, scripted};
use crate::error::Error;

fn limit(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("a limit")
}

async fn page(world: &World, after: Option<&Cursor>, n: usize) -> InboxBatch {
    let agent = world.agent().await;
    let context = world.context(agent);
    tokio::time::timeout(
        Duration::from_secs(5),
        world.inbox().items(&context, after, limit(n)),
    )
    .await
    .expect("the listing ends")
    .expect("a batch")
}

#[tokio::test]
async fn given_five_unread_items_when_they_are_listed_in_batches_of_two_then_jobs_order_and_a_cursor_carry_the_pages_from_one_read()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let items = (1..=5).map(|n| cc_item(agent.as_uuid(), n)).collect();
    world.jobs.graphql_default(only_unread(items));
    let first = page(&world, None, 2).await;
    let second = page(&world, first.next.as_ref(), 2).await;
    let third = page(&world, second.next.as_ref(), 2).await;
    let ids = |batch: &InboxBatch| {
        batch
            .items
            .iter()
            .map(|item| item.id().as_uuid())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&first), vec![id(201), id(202)]);
    assert_eq!(ids(&second), vec![id(203), id(204)]);
    assert_eq!(ids(&third), vec![id(205)]);
    assert!(first.next.is_some() && second.next.is_some() && third.next.is_none());
    assert_ne!(first.next, second.next);
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
}

#[tokio::test]
async fn given_a_cursor_the_adapter_no_longer_holds_when_the_next_page_is_asked_then_one_new_read_positions_after_the_item_it_names()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let items = (1..=3).map(|n| cc_item(agent.as_uuid(), n)).collect();
    world.jobs.graphql_default(only_unread(items));
    let cursor = Cursor::new(id(101).to_string());
    let batch = page(&world, Some(&cursor), 10).await;
    let found: Vec<_> = batch.items.iter().map(|i| i.id().as_uuid()).collect();
    assert_eq!(found, vec![id(202), id(203)]);
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
}

#[tokio::test]
async fn given_a_cursor_that_is_not_an_item_id_when_the_page_is_asked_then_the_error_is_a_fault_and_jobs_is_not_called()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let context = world.context(agent);
    let cursor = Cursor::new("not a position");
    let error = world
        .inbox()
        .items(&context, Some(&cursor), limit(2))
        .await
        .expect_err("an error");
    assert!(!error.is_refusal());
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 0);
}

#[tokio::test]
async fn given_a_new_agent_whose_registration_is_queued_when_it_is_listed_then_the_inbox_is_empty_and_jobs_gets_no_call()
 {
    let jobs = scripted();
    jobs.answer_facts(|| Err(Error::lost("the started fact is not delivered")));
    let world = World::build(jobs, Vec::new(), mark_gate_stored()).await;
    let agent = world.agent().await;
    let items = vec![cc_item(agent.as_uuid(), 1)];
    world.jobs.graphql_default(only_unread(items));
    let batch = page(&world, None, 5).await;
    assert!(batch.items.is_empty() && batch.next.is_none());
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 0);
    assert_eq!(count(&world.jobs, MAIL_ROOT), 0);
    world.jobs.answer_facts(|| Ok(()));
    drained(&world.queue).await;
    let batch = page(&world, None, 5).await;
    assert_eq!(batch.items.len(), 1);
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
}

#[tokio::test]
async fn given_a_restored_agent_whose_registration_is_queued_when_it_is_listed_then_jobs_is_read_at_once()
 {
    let jobs = scripted();
    jobs.answer_facts(|| Err(Error::lost("the started fact is not delivered")));
    let restored = super::world::restored_writer();
    let world = World::build(jobs, vec![restored], mark_gate_stored()).await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(id(70), 1)]));
    let batch = page(&world, None, 5).await;
    assert_eq!(batch.items.len(), 1);
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
}

#[tokio::test]
async fn given_a_direct_mail_when_it_is_listed_then_the_recipient_lists_come_from_one_read_of_the_mail_and_the_mail_is_not_read_again()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let other = id(71);
    let reply = wire(
        vec![mail_item(agent.as_uuid(), 1)],
        mail_view(1, vec![agent.as_uuid(), other], vec![id(72)]),
        attachment_view("f.txt", "x"),
    );
    world.jobs.graphql_default(reply);
    let first = page(&world, None, 5).await;
    let second = page(&world, None, 5).await;
    assert_eq!(first, second);
    let InboxItem::Mail(MailItem::To {
        id: mail,
        sender,
        author,
        source,
        to,
        cc,
        subject,
        body,
        attachment_names,
    }) = &first.items[0]
    else {
        panic!("{first:?}");
    };
    assert_eq!(mail.as_uuid(), id(201));
    assert_eq!(sender.as_uuid(), id(5));
    assert_eq!(author.as_str(), "Ann");
    assert_eq!(source.as_uuid(), id(6));
    assert_eq!(
        to.iter().map(|a| a.as_uuid()).collect::<Vec<_>>(),
        vec![agent.as_uuid(), other]
    );
    assert_eq!(
        cc.iter().map(|a| a.as_uuid()).collect::<Vec<_>>(),
        vec![id(72)]
    );
    assert_eq!((subject.as_str(), body.as_str()), ("subject", "body"));
    assert_eq!(attachment_names, &vec!["f.txt".to_owned()]);
    assert_eq!(count(&world.jobs, MAIL_ROOT), 1);
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 2);
}

#[tokio::test]
async fn given_a_cc_notice_when_it_is_listed_then_the_item_has_the_mail_id_and_the_subject_and_no_read_of_the_mail_is_made()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    let batch = page(&world, None, 5).await;
    let InboxItem::Mail(MailItem::Cc { id: mail, subject }) = &batch.items[0] else {
        panic!("{batch:?}");
    };
    assert_eq!(mail.as_uuid(), id(201));
    assert_eq!(subject, "cc subject");
    assert_eq!(count(&world.jobs, MAIL_ROOT), 0);
}

#[tokio::test]
async fn given_a_direct_mail_that_cannot_be_made_into_an_item_then_the_listing_is_an_error_of_the_port_and_nothing_is_read_again()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let mut item = mail_item(agent.as_uuid(), 1);
    item["sourceConversationId"] = serde_json::Value::Null;
    let reply = wire(
        vec![item],
        mail_view(1, vec![agent.as_uuid()], Vec::new()),
        attachment_view("f.txt", "x"),
    );
    world.jobs.graphql_default(reply);
    let context = world.context(agent);
    let error = world
        .inbox()
        .items(&context, None, limit(5))
        .await
        .expect_err("an error");
    assert!(!error.is_refusal());
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
    assert!(world.book.listed(agent.as_uuid(), id(201)).is_none());
}

#[tokio::test]
async fn given_a_direct_mail_that_does_not_list_the_agent_in_to_then_it_is_an_error_of_the_port() {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let reply = wire(
        vec![mail_item(agent.as_uuid(), 1)],
        mail_view(1, vec![id(71)], vec![agent.as_uuid()]),
        attachment_view("f.txt", "x"),
    );
    world.jobs.graphql_default(reply);
    let context = world.context(agent);
    assert!(world.inbox().items(&context, None, limit(5)).await.is_err());
}

#[tokio::test]
async fn given_a_listing_that_gets_no_answer_from_jobs_then_the_error_is_a_fault_of_the_port_and_jobs_is_read_once()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.push_graphql(Err(Error::wait_fired("no answer")));
    let context = world.context(agent);
    let error = world
        .inbox()
        .items(&context, None, limit(5))
        .await
        .expect_err("an error");
    assert!(!error.is_refusal());
    yields().await;
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
}

#[tokio::test]
async fn given_a_listing_dropped_while_jobs_is_slow_then_nothing_is_recorded_and_the_next_listing_gives_every_item()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    world.jobs.pause();
    let context = world.context(agent);
    let inbox = world.inbox();
    let mut listing = inbox.items(&context, None, limit(5));
    assert!(listing.as_mut().now_or_never().is_none());
    yields().await;
    drop(listing);
    assert!(world.book.listed(agent.as_uuid(), id(201)).is_none());
    world.jobs.resume();
    let batch = page(&world, None, 5).await;
    assert_eq!(batch.items.len(), 1);
    assert!(world.book.listed(agent.as_uuid(), id(201)).is_some());
}

#[tokio::test]
async fn given_a_listed_item_when_the_adapter_records_it_then_the_book_maps_the_entry_id_to_the_item_of_jobs()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 3)]));
    page(&world, None, 5).await;
    let entry = world
        .book
        .listed(agent.as_uuid(), id(203))
        .expect("a recorded entry");
    assert_eq!(entry.item_id, id(103));
    assert!(entry.child_job_id.is_none());
}
