use std::num::NonZeroUsize;
use std::time::Duration;

use br_agentic_contract::{Cursor, IncomingBatch, IncomingPort};
use contract_jobs::graphql_roots::{MAIL_ROOT, UNREAD_ROOT};

use super::support::id;
use super::{World, attachment_view, cc_item, count, mail_item, mail_view, only_unread, wire};
use crate::adapter::mark_gate_stored;

async fn page(world: &World, after: Option<&Cursor>, n: usize) -> IncomingBatch {
    let agent = world.agent().await;
    let context = world.context(agent);
    tokio::time::timeout(
        Duration::from_secs(5),
        world
            .incoming()
            .items(&context, after, NonZeroUsize::new(n).expect("a limit")),
    )
    .await
    .expect("the listing ends")
    .expect("a batch")
}

#[tokio::test]
async fn given_three_new_direct_mails_when_they_are_listed_then_jobs_is_read_once_for_the_unread_items_and_once_for_each_mail_and_no_more()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let items = (1..=3).map(|n| mail_item(agent.as_uuid(), n)).collect();
    let reply = wire(
        items,
        mail_view(1, vec![agent.as_uuid()], Vec::new()),
        attachment_view("f.txt", "x"),
    );
    world.jobs.graphql_default(reply);
    let batch = page(&world, None, 10).await;
    assert_eq!(batch.items.len(), 3);
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
    assert_eq!(count(&world.jobs, MAIL_ROOT), 3);
    let again = page(&world, None, 10).await;
    assert_eq!(again.items.len(), 3);
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 2);
    assert_eq!(count(&world.jobs, MAIL_ROOT), 3);
}

#[tokio::test]
async fn given_a_cursor_whose_item_is_gone_and_items_whose_ids_are_not_in_the_order_of_jobs_when_the_next_page_is_asked_then_one_fresh_read_gives_them_in_the_order_of_jobs_and_no_id_is_compared()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let in_order_of_jobs = [50_u128, 20, 40];
    let items = in_order_of_jobs
        .iter()
        .enumerate()
        .map(|(position, n)| {
            let mut item = cc_item(agent.as_uuid(), *n);
            item["seq"] = serde_json::json!(position + 1);
            item
        })
        .collect();
    world.jobs.graphql_default(only_unread(items));
    let cursor = Cursor::new(id(130).to_string());
    let batch = page(&world, Some(&cursor), 10).await;
    let found: Vec<_> = batch.items.iter().map(|i| i.id().as_uuid()).collect();
    assert_eq!(found, vec![id(250), id(220), id(240)]);
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
}

#[tokio::test]
async fn given_a_cursor_whose_item_is_still_unread_when_the_next_page_is_asked_then_the_page_goes_on_after_it_in_the_order_of_jobs_whatever_the_item_ids()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let items = [50_u128, 20, 40]
        .iter()
        .map(|n| cc_item(agent.as_uuid(), *n))
        .collect();
    world.jobs.graphql_default(only_unread(items));
    let cursor = Cursor::new(id(150).to_string());
    let batch = page(&world, Some(&cursor), 10).await;
    let found: Vec<_> = batch.items.iter().map(|i| i.id().as_uuid()).collect();
    assert_eq!(found, vec![id(220), id(240)]);
}
