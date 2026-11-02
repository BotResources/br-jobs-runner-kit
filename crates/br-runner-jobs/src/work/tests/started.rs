use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::error::{Cause, Error, Failure, Operation};
use crate::queue::Removal;
use crate::reason::RefusalReason;
use crate::test_double::Call;

fn started_calls(jobs: &ScriptedJobs) -> Vec<(Uuid, Uuid)> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Started { run_id, start_id } => Some((run_id, start_id)),
            _ => None,
        })
        .collect()
}

fn refusal(reason: RefusalReason) -> Error {
    Error::refused(Operation::StartedFact, reason, "in plain words")
}

fn answering(jobs: &ScriptedJobs, answers: Vec<Result<(), Error>>) -> Arc<AtomicUsize> {
    let taken = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&taken);
    jobs.answer_started(move || {
        let at = counter.fetch_add(1, Ordering::SeqCst);
        answers.get(at).cloned().unwrap_or(Ok(()))
    });
    taken
}

#[tokio::test]
async fn given_a_refusal_other_than_instance_unknown_when_the_start_is_sent_then_it_is_final_sent_once_and_removed_as_refused()
 {
    for reason in [
        RefusalReason::RunHeld,
        RefusalReason::RunReservedElsewhere,
        RefusalReason::RunNotPending,
        RefusalReason::RunUnknown,
        RefusalReason::Malformed,
    ] {
        let jobs = jobs();
        answering(&jobs, vec![Err(refusal(reason.clone()))]);
        let work = work(&jobs);
        let (_queue, started) = work.delivery_queue(settings()).await.expect("a queue");
        match started.await {
            Err(Removal::Refused(Error::Refused(refused))) => {
                assert_eq!(refused.operation, Operation::StartedFact);
                assert_eq!(refused.reason, reason);
            }
            other => panic!("{reason}: {other:?}"),
        }
        assert_eq!(started_calls(&jobs).len(), 1, "{reason}");
    }
}

#[tokio::test]
async fn given_a_refusal_instance_unknown_when_the_start_is_sent_then_it_is_sent_again_with_the_same_start_id_until_accepted()
 {
    let jobs = jobs();
    answering(
        &jobs,
        vec![
            Err(refusal(RefusalReason::InstanceUnknown)),
            Err(refusal(RefusalReason::InstanceUnknown)),
            Ok(()),
        ],
    );
    let work = work(&jobs);
    let (_queue, started) = work.delivery_queue(settings()).await.expect("a queue");
    assert_eq!(started.await, Ok(()));
    let calls = started_calls(&jobs);
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|call| *call == (id(RUN), id(START))));
}

#[tokio::test]
async fn given_a_start_that_is_lost_or_unavailable_when_it_is_sent_then_it_is_sent_again_with_the_same_start_id()
 {
    let jobs = jobs();
    answering(
        &jobs,
        vec![
            Err(Error::Lost(Failure::new(Cause::WaitFired, "no reply"))),
            Err(Error::Unavailable(Failure::new(
                Cause::NoListener,
                "nobody",
            ))),
            Err(Error::Unreadable("the reply cannot be read".to_owned())),
            Ok(()),
        ],
    );
    let work = work(&jobs);
    let (_queue, started) = work.delivery_queue(settings()).await.expect("a queue");
    assert_eq!(started.await, Ok(()));
    let calls = started_calls(&jobs);
    assert_eq!(calls.len(), 4);
    assert!(calls.iter().all(|call| *call == (id(RUN), id(START))));
}

#[tokio::test]
async fn given_a_refused_start_when_a_write_follows_it_then_the_write_depends_on_the_removed_start()
{
    let jobs = jobs();
    answering(&jobs, vec![Err(refusal(RefusalReason::RunHeld))]);
    let work = work(&jobs);
    let (queue, started) = work.delivery_queue(settings()).await.expect("a queue");
    let completed = queue.completed();
    assert!(matches!(started.await, Err(Removal::Refused(_))));
    assert_eq!(completed.await, Err(Removal::DependsOnRemoved));
}
