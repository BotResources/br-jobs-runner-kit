use std::sync::Arc;

use super::*;
use crate::book::{AgentKey, BookedAgent};
use crate::error::Error;
use crate::model::AgentDeclaration;
use crate::test_double::Call;

fn restored(n: u128, inbox: bool) -> RestoredAgent {
    RestoredAgent {
        key: AgentKey::new("node", format!("label-{n}")),
        agent_id: id(100 + n),
        has_inbox: inbox,
    }
}

fn registrations(jobs: &ScriptedJobs) -> Vec<AgentDeclaration> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Register { agents, .. } => Some(agents),
            _ => None,
        })
        .flatten()
        .collect()
}

#[tokio::test]
async fn given_restored_agents_when_the_book_is_made_then_each_registration_follows_the_started_fact_with_the_book_wide_restart_flag()
 {
    let jobs = scripted();
    let fixture = fixture_on(
        jobs.clone(),
        vec![restored(1, true), restored(2, false)],
        true,
    )
    .await;
    fixture.queue.drained().await;
    let calls = jobs.calls();
    assert!(matches!(
        calls[0],
        Call::Started { run_id, start_id } if run_id == id(RUN) && start_id == id(START)
    ));
    assert_eq!(
        registrations(&jobs),
        vec![
            AgentDeclaration {
                agent_id: id(101),
                has_inbox: true,
                can_restart: true
            },
            AgentDeclaration {
                agent_id: id(102),
                has_inbox: false,
                can_restart: true
            },
        ]
    );
}

#[tokio::test]
async fn given_a_jobs_that_never_answers_when_the_book_is_made_then_the_making_does_not_wait_and_the_agents_are_listed()
 {
    let jobs = scripted();
    jobs.pause();
    let fixture = fixture_on(jobs.clone(), vec![restored(1, true)], false).await;
    assert_eq!(
        fixture.book.agents(),
        vec![BookedAgent {
            key: AgentKey::new("node", "label-1"),
            agent_id: id(101),
            has_inbox: true,
        }]
    );
    assert!(registrations(&jobs).is_empty());
    jobs.resume();
    fixture.queue.drained().await;
    assert_eq!(registrations(&jobs).len(), 1);
}

#[tokio::test]
async fn given_a_book_made_when_a_second_book_is_made_from_the_same_work_then_the_second_is_invalid_input()
 {
    let fixture = fixture().await;
    let second = fixture.work.agent_book(&fixture.queue, Vec::new(), false);
    assert!(matches!(second, Err(Error::InvalidInput(_))), "{second:?}");
}

#[tokio::test]
async fn given_a_restored_list_with_a_repeated_key_when_the_book_is_made_then_it_is_invalid_input_and_the_once_is_kept()
 {
    let jobs = scripted();
    let work = work(&jobs);
    let (queue, _) = work.delivery_queue(settings()).await.expect("a queue");
    let twice = vec![restored(1, true), restored(1, false)];
    assert!(matches!(
        work.agent_book(&queue, twice, false),
        Err(Error::InvalidInput(_))
    ));
    let same_id = vec![
        restored(1, true),
        RestoredAgent {
            key: AgentKey::new("node", "other"),
            agent_id: id(101),
            has_inbox: false,
        },
    ];
    assert!(matches!(
        work.agent_book(&queue, same_id, false),
        Err(Error::InvalidInput(_))
    ));
    assert!(
        work.agent_book(&queue, vec![restored(1, true)], false)
            .is_ok()
    );
}

#[tokio::test]
async fn given_a_new_key_when_the_agent_is_asked_for_then_a_uuid_v7_is_minted_and_registered_once_with_its_inbox_flag()
 {
    let fixture = fixture().await;
    let key = AgentKey::new("node", "writer");
    let first = fixture.book.agent_for(&key, true);
    let second = fixture.book.agent_for(&key, false);
    assert!(first.created && !second.created);
    assert_eq!(first.agent_id, second.agent_id);
    assert_eq!(first.agent_id.get_version_num(), 7);
    assert!(first.has_inbox && second.has_inbox);
    assert!(fixture.book.holds_inbox(first.agent_id));
    fixture.queue.drained().await;
    let declared = registrations(&fixture.jobs);
    assert_eq!(declared.len(), 1);
    assert_eq!(declared[0].agent_id, first.agent_id);
    assert!(declared[0].has_inbox);
}

#[tokio::test]
async fn given_a_label_without_inbox_when_the_agent_is_made_then_it_holds_no_inbox() {
    let fixture = fixture().await;
    let agent = fixture
        .book
        .agent_for(&AgentKey::new("node", "helper"), false);
    assert!(!agent.has_inbox);
    assert!(!fixture.book.holds_inbox(agent.agent_id));
    assert!(!fixture.book.holds_inbox(id(999)));
}

#[tokio::test]
async fn given_a_restored_key_when_the_agent_is_asked_for_then_the_restored_id_and_inbox_answer_without_a_new_registration()
 {
    let jobs = scripted();
    let fixture = fixture_on(jobs.clone(), vec![restored(1, false)], false).await;
    let agent = fixture
        .book
        .agent_for(&AgentKey::new("node", "label-1"), true);
    assert_eq!(agent.agent_id, id(101));
    assert!(!agent.has_inbox && !agent.created);
    fixture.queue.drained().await;
    assert_eq!(registrations(&jobs).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn given_calls_at_the_same_time_for_one_key_when_the_agent_is_asked_for_then_one_agent_is_made()
 {
    let jobs = scripted();
    let fixture = fixture_on(jobs.clone(), Vec::new(), false).await;
    let book = Arc::new(fixture.book);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let book = Arc::clone(&book);
            std::thread::spawn(move || book.agent_for(&AgentKey::new("node", "shared"), true))
        })
        .collect();
    let ids: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().expect("a thread").agent_id)
        .collect();
    assert!(ids.iter().all(|found| *found == ids[0]));
    assert_eq!(book.agents().len(), 1);
    fixture.queue.drained().await;
    assert_eq!(registrations(&jobs).len(), 1);
}

#[tokio::test]
async fn given_a_registration_when_it_ends_then_the_shared_end_wakes_every_holder() {
    let fixture = fixture().await;
    let agent = fixture.book.agent_for(&AgentKey::new("node", "a"), true);
    let again = fixture
        .book
        .registration_end(agent.agent_id)
        .expect("an end");
    assert_eq!(agent.registration.await, Ok(()));
    assert_eq!(again.await, Ok(()));
    assert!(fixture.book.registration_end(id(999)).is_none());
}

#[tokio::test]
async fn given_a_book_with_a_queued_agent_then_the_saved_state_list_has_it_before_its_registration_is_delivered()
 {
    let jobs = scripted();
    jobs.pause();
    let fixture = fixture_on(jobs.clone(), Vec::new(), false).await;
    let agent = fixture.book.agent_for(&AgentKey::new("node", "q"), true);
    assert_eq!(fixture.book.agents()[0].agent_id, agent.agent_id);
    jobs.resume();
}
