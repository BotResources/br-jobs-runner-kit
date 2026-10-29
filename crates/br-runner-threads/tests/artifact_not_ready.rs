mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use br_llm_messages::TurnEntry;
use br_runner_threads::transport::{Answer, Confirmed, Reply};
use br_runner_threads::{Annotation, AnnotationBody, ClosureKind, Error, MessageStatus, Refusal};
use support::closings::seals_sent;
use support::*;

#[tokio::test]
async fn an_artifact_that_is_not_ready_refuses_the_completion_the_message_stays_open_and_a_later_completion_is_confirmed()
 {
    let world = world();
    let handle = open(&world).await;
    let entry = notice();
    let entry_id = match &entry {
        TurnEntry::RuntimeNotice(notice) => notice.id(),
        _ => unreachable!(),
    };
    handle.entry(&entry).await.expect("entry");
    let artifact = Annotation::new(v7(), AnnotationBody::Artifact { artifact_id: v7() });
    handle
        .annotation(entry_id, &artifact)
        .await
        .expect("the annotation chunk is accepted");

    let completions = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&completions);
    world.bus.answer_with(move |command| {
        if command.verb != "complete" {
            return Reply::Answer(Answer::Confirmed(Confirmed {
                command_id: command.command_id,
                status: None,
            }));
        }
        if seen.fetch_add(1, Ordering::SeqCst) == 0 {
            return Reply::Answer(Answer::Refused(Refusal {
                code: "ARTIFACT_NOT_READY".to_owned(),
                params: serde_json::Map::new(),
                command_id: Some(command.command_id),
            }));
        }
        Reply::Answer(Answer::Confirmed(Confirmed {
            command_id: command.command_id,
            status: Some(MessageStatus::Completed),
        }))
    });

    let first = closing(&handle, world.thread, ClosureKind::Completed);
    match world.client.close_message(&first, Some(&handle)).await {
        Err(Error::Refused(refusal)) => assert_eq!(refusal.code, "ARTIFACT_NOT_READY"),
        other => panic!("refused, not {other:?}"),
    }

    handle
        .keepalive()
        .await
        .expect("the message is still open: the handle takes a chunk");

    let second = closing(&handle, world.thread, ClosureKind::Completed);
    let confirmed = world
        .client
        .close_message(&second, Some(&handle))
        .await
        .expect("the completion is confirmed once the artifact is ready");
    assert_eq!(confirmed.status, Some(MessageStatus::Completed));

    let seals = seals_sent(&world);
    assert_eq!((seals[0].last_seq, seals[1].last_seq), (1, 2));
    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}
