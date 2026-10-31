mod support;

use std::time::Duration;

use br_runner_threads::test_double::{CommandBody, MemoryThreads};
use br_runner_threads::{Error, ThreadOrigin};
use support::client_x::*;
use support::*;

#[tokio::test]
async fn creating_a_thread_sends_every_value_the_importer_gave() {
    let threads = MemoryThreads::new();
    let mut thread = new_thread();
    let (owner, member, parent, block, acting) = (v7(), v7(), v7(), v7(), v7());
    thread.owner_id = Some(owner);
    thread.member_ids = vec![member];
    thread.title = Some("Quarterly review".to_owned());
    thread
        .resources
        .insert("kms".to_owned(), vec!["a".to_owned(), "b".to_owned()]);
    thread.origin = Some(ThreadOrigin {
        parent_thread_id: parent,
        origin_block_id: block,
    });
    thread.acting_user_id = Some(acting);

    let confirmed = threads
        .client()
        .create_thread(&thread)
        .await
        .expect("confirmed");

    let command = last_command(&threads);
    let CommandBody::CreateThread(create) = &command.body else {
        panic!("not a create command");
    };
    assert_eq!(command.id, thread.command_id);
    assert_eq!(create.thread_id, thread.thread_id);
    assert_eq!(create.owner_id, Some(owner));
    assert_eq!(create.user_ids, vec![member]);
    assert_eq!(create.title.as_deref(), Some("Quarterly review"));
    assert_eq!(
        create.resources["kms"],
        vec!["a".to_owned(), "b".to_owned()]
    );
    assert_eq!(create.parent_thread_id, Some(parent));
    assert_eq!(create.origin_block_id, Some(block));
    assert_eq!(create.acting_user_id, Some(acting));
    assert_eq!(confirmed.command_id, thread.command_id);
    assert_eq!(confirmed.status, None);
}

#[tokio::test]
async fn creating_a_thread_without_optional_values_sends_none_of_them() {
    let threads = MemoryThreads::new();
    let thread = new_thread();

    threads
        .client()
        .create_thread(&thread)
        .await
        .expect("confirmed");

    let CommandBody::CreateThread(create) = last_command(&threads).body else {
        panic!("not a create command");
    };
    assert_eq!(create.owner_id, None);
    assert!(create.user_ids.is_empty());
    assert_eq!(create.title, None);
    assert_eq!(create.parent_thread_id, None);
    assert_eq!(create.origin_block_id, None);
    assert_eq!(create.acting_user_id, None);
}

#[tokio::test]
async fn a_thread_id_that_is_not_a_uuidv7_is_refused_before_sending() {
    let threads = MemoryThreads::new();
    let mut thread = new_thread();
    thread.thread_id = not_v7();

    let result = threads.client().create_thread(&thread).await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert!(threads.operations().is_empty());
}

#[tokio::test]
async fn a_command_id_that_is_not_a_uuidv7_is_refused_before_sending() {
    let threads = MemoryThreads::new();
    let client = threads.client();
    let mut thread = new_thread();
    thread.command_id = uuid::Uuid::nil();
    let mut message = ai_message(v7());
    message.command_id = not_v7();
    let mut open = open_request(v7());
    open.command_id = not_v7();

    let created = client.create_thread(&thread).await;
    let written = client.write_message(&message).await;
    let opened = client.open_stream(&open).await;

    assert!(matches!(created, Err(Error::InvalidInput(_))));
    assert!(matches!(written, Err(Error::InvalidInput(_))));
    assert!(matches!(opened, Err(Error::InvalidInput(_))));
    assert!(threads.operations().is_empty());
}

#[tokio::test]
async fn a_zero_outcome_wait_is_refused_with_invalid_input_for_every_command() {
    let threads = MemoryThreads::new();
    let client = threads.client();
    let mut thread = new_thread();
    thread.wait = Duration::ZERO;
    let mut message = ai_message(v7());
    message.wait = Duration::ZERO;
    let mut open = open_request(v7());
    open.wait = Duration::ZERO;
    let (handle, opened_request) = opened(&client).await;
    let mut closing = closing_of(&opened_request, br_runner_threads::ClosureKind::Cancelled);
    closing.wait = Duration::ZERO;
    let sent_before = threads.operations().len();

    let created = client.create_thread(&thread).await;
    let written = client.write_message(&message).await;
    let opened_again = client.open_stream(&open).await;
    let closed = client.close_message(&closing, Some(&handle)).await;

    assert!(matches!(created, Err(Error::InvalidInput(_))));
    assert!(matches!(written, Err(Error::InvalidInput(_))));
    assert!(matches!(opened_again, Err(Error::InvalidInput(_))));
    assert!(matches!(closed, Err(Error::InvalidInput(_))));
    assert_eq!(threads.operations().len(), sent_before);
}
