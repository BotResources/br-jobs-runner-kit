use std::sync::Arc;

use br_agentic_contract::{AgentLabel, AgentPort, PortError};

use super::world::restored_writer;
use super::{Snap, World, drained, instance, label, yields};
use crate::adapter::{NodeEntry, book_lookup, mark_gate_stored};
use crate::book::AgentKey;
use crate::book::tests::{id, scripted};
use crate::error::Error;
use crate::model::AgentDeclaration;
use crate::test_double::Call;

fn registered(world: &World) -> Vec<AgentDeclaration> {
    world
        .jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Register { agents, .. } => Some(agents),
            _ => None,
        })
        .flatten()
        .collect()
}

#[tokio::test]
async fn given_a_new_key_when_the_agent_port_is_called_then_a_uuid_v7_comes_back_at_once_while_jobs_is_paused()
 {
    let world = World::new(mark_gate_stored()).await;
    world.jobs.pause();
    let agent = world.agent().await;
    assert_eq!(agent.as_uuid().get_version_num(), 7);
    assert!(registered(&world).is_empty());
    world.jobs.resume();
    drained(&world.queue).await;
    assert_eq!(registered(&world).len(), 1);
}

#[tokio::test]
async fn given_a_key_when_the_agent_port_is_called_twice_then_the_id_is_the_same_and_the_registration_is_queued_once()
 {
    let world = World::new(mark_gate_stored()).await;
    let first = world.agent().await;
    let second = world.agent().await;
    assert_eq!(first, second);
    drained(&world.queue).await;
    assert_eq!(registered(&world).len(), 1);
}

#[tokio::test]
async fn given_the_label_that_holds_the_inbox_then_the_registration_has_an_inbox_and_any_other_label_has_none()
 {
    let world = World::new(mark_gate_stored()).await;
    let port = world.entry.agent_port();
    let writer = port
        .agent(&Snap, instance(), &label())
        .await
        .expect("agent");
    let other = AgentLabel::new("helper").expect("label");
    let helper = port.agent(&Snap, instance(), &other).await.expect("agent");
    drained(&world.queue).await;
    assert_ne!(writer, helper);
    let declared = registered(&world);
    let has_inbox = |agent: br_llm_messages::AgentId| {
        declared
            .iter()
            .find(|declaration| declaration.agent_id == agent.as_uuid())
            .map(|declaration| declaration.has_inbox)
    };
    assert_eq!(has_inbox(writer), Some(true));
    assert_eq!(has_inbox(helper), Some(false));
}

#[tokio::test]
async fn given_concurrent_first_calls_for_one_key_then_one_agent_is_made_and_registered_once() {
    let world = World::new(mark_gate_stored()).await;
    let port = world.entry.agent_port();
    let label = label();
    let calls = (0..16).map(|_| port.agent(&Snap, instance(), &label));
    let agents = futures::future::join_all(calls).await;
    let first = agents[0].as_ref().expect("agent").as_uuid();
    assert!(
        agents
            .iter()
            .all(|agent| agent.as_ref().expect("agent").as_uuid() == first)
    );
    drained(&world.queue).await;
    assert_eq!(registered(&world).len(), 1);
    assert_eq!(world.book.agents().len(), 1);
}

#[tokio::test]
async fn given_the_node_entry_and_the_port_when_both_ask_for_one_key_then_they_share_the_agent() {
    let world = World::new(mark_gate_stored()).await;
    let from_entry = world.entry.agent_id(&world.book, &label()).expect("agent");
    let from_port = world.agent().await;
    assert_eq!(from_entry, from_port);
    drained(&world.queue).await;
    assert_eq!(registered(&world).len(), 1);
}

#[tokio::test]
async fn given_a_node_name_then_the_same_label_of_another_node_is_another_agent() {
    let world = World::new(mark_gate_stored()).await;
    let book = Arc::clone(&world.book);
    let other = NodeEntry::new(
        "other-node",
        book_lookup(move |_| {
            let book = Arc::clone(&book);
            async move { Ok(book) }
        }),
    );
    let first = world.agent().await;
    let second = other
        .agent_port()
        .agent(&Snap, instance(), &label())
        .await
        .expect("agent");
    assert_ne!(first, second);
    assert!(
        world
            .book
            .agents()
            .iter()
            .any(|booked| booked.key == AgentKey::new("other-node", "writer") && !booked.has_inbox)
    );
}

#[tokio::test]
async fn given_a_restored_agent_when_the_port_is_called_then_the_saved_id_comes_back_and_nothing_is_registered_again()
 {
    let world = World::build(scripted(), vec![restored_writer()], mark_gate_stored()).await;
    drained(&world.queue).await;
    let before = registered(&world).len();
    let agent = world.agent().await;
    drained(&world.queue).await;
    assert_eq!(agent.as_uuid(), id(70));
    assert_eq!(registered(&world).len(), before);
}

#[tokio::test]
async fn given_a_lookup_that_fails_when_the_port_is_called_then_the_error_of_the_lookup_is_the_error_of_the_port()
 {
    let lookup = book_lookup(|_| async { Err(PortError::fault("no run for this instance")) });
    let entry = NodeEntry::new("node", lookup);
    let error = entry
        .agent_port()
        .agent(&Snap, instance(), &label())
        .await
        .expect_err("an error");
    assert!(!error.is_refusal());
    yields().await;
}

#[test]
fn given_a_node_without_inbox_then_it_has_no_inbox_adapter_and_a_node_with_inbox_has_one() {
    let lookup = book_lookup(|_| async { Err(PortError::fault("unused")) });
    assert!(
        NodeEntry::new("node", lookup.clone())
            .inbox_port()
            .is_none()
    );
    let with = NodeEntry::with_inbox("node", lookup, label(), mark_gate_stored());
    assert!(with.inbox_port().is_some());
}

#[tokio::test]
async fn given_a_lookup_that_fails_when_any_inbox_call_is_made_then_each_is_an_error_of_the_port() {
    use br_agentic_contract::{InboxContext, InboxPort, MailId};
    use std::num::NonZeroUsize;

    let lookup = book_lookup(|_| async { Err(PortError::fault("no run for this instance")) });
    let entry = NodeEntry::with_inbox("node", lookup, label(), mark_gate_stored());
    let inbox = entry.inbox_port().expect("an inbox");
    let context = InboxContext {
        instance: instance(),
        thread: super::thread(),
        agent: br_llm_messages::AgentId::from_uuid(id(70)).expect("agent"),
    };
    let one = NonZeroUsize::new(1).expect("limit");
    let mail = MailId::from_uuid(id(201)).expect("mail");
    let entry_id = br_llm_messages::EntryId::from_uuid(id(201)).expect("entry");
    assert!(inbox.items(&context, None, one).await.is_err());
    assert!(inbox.wait(&context).await.is_err());
    assert!(inbox.mark_read(&context, entry_id).await.is_err());
    assert!(inbox.documents(&context, mail).await.is_err());
}

#[tokio::test]
async fn given_a_restored_agent_id_that_is_not_a_uuid_v7_then_the_book_is_not_made() {
    let jobs = scripted();
    let work = crate::book::tests::work(&jobs);
    let (queue, _) = work
        .delivery_queue(crate::book::tests::settings())
        .await
        .expect("a queue");
    let mut restored = restored_writer();
    restored.agent_id = uuid::Uuid::nil();
    assert!(matches!(
        work.agent_book(&queue, vec![restored], false),
        Err(Error::InvalidInput(_))
    ));
}
