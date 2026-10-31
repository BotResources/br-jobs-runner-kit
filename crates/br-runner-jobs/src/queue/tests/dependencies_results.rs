use super::support::*;
use crate::error::{Error, Operation};
use crate::queue::{Removal, WriteKind};

fn refused(operation: Operation) -> Error {
    refusal(operation, "refused_by_jobs")
}

#[tokio::test]
async fn given_a_removed_answer_or_saved_state_when_the_completed_fact_waits_then_it_depends_on_it_for_any_cause()
 {
    let causes = [
        refused(Operation::Result),
        Error::Unauthenticated,
        no_usable(),
    ];
    for cause in causes {
        for removed in [answer(), saved()] {
            let sender = ScriptedSender::new();
            sender.script_write(&removed, vec![fail(cause.clone())]);
            let rig = Rig::with(sender);
            let answer_end = rig.hand(answer());
            let saved_end = rig.hand(saved());
            let completed_end = rig.hand(completed());
            let failed_end = rig.hand(failed_fact());
            let _ = answer_end.await;
            let _ = saved_end.await;
            assert_eq!(completed_end.await, Err(Removal::DependsOnRemoved));
            assert_eq!(failed_end.await, Ok(()));
            assert!(rig.sender.attempts_of(WriteKind::Completed).is_empty());
            let mut late = rig.hand(completed());
            assert_eq!(ready(&mut late), Some(Err(Removal::DependsOnRemoved)));
        }
    }
}

#[tokio::test]
async fn given_a_delivered_answer_and_saved_state_or_none_when_the_completed_fact_is_handed_over_then_it_is_sent_last()
 {
    let rig = Rig::new();
    let ends = vec![rig.hand(answer()), rig.hand(saved()), rig.hand(completed())];
    for end in ends {
        assert_eq!(end.await, Ok(()));
    }
    assert_eq!(rig.sender.kinds().last(), Some(&WriteKind::Completed));
    let bare = Rig::new();
    assert_eq!(bare.hand(completed()).await, Ok(()));
}

#[tokio::test]
async fn given_a_refused_upload_when_the_create_waits_then_the_create_is_removed_as_refused_with_the_refusal_of_the_upload()
 {
    let upload_refusal = refusal(Operation::Upload, "size");
    let sender = ScriptedSender::new();
    sender.script(WriteKind::Upload, vec![fail(upload_refusal.clone())]);
    let rig = Rig::with(sender);
    let up = rig.hand(upload(41));
    let queued = rig.hand(create(41, 42));
    let other_job = rig.hand(create(51, 52));
    assert_eq!(up.await, Err(Removal::Refused(upload_refusal.clone())));
    assert_eq!(queued.await, Err(Removal::Refused(upload_refusal.clone())));
    assert_eq!(other_job.await, Ok(()));
    let mut late = rig.hand(create(41, 43));
    assert_eq!(
        ready(&mut late),
        Some(Err(Removal::Refused(upload_refusal.clone())))
    );
    assert_eq!(
        rig.sender.attempts_of(WriteKind::Create),
        vec![create(51, 52)]
    );
    let after = rig.hand(finish(41, 44));
    assert_eq!(after.await, Err(Removal::DependsOnRemoved));
}

#[tokio::test]
async fn given_an_upload_removed_for_another_cause_when_the_create_waits_then_the_create_depends_on_it()
 {
    for cause in [Error::Unauthenticated, no_usable(), Error::invalid("size")] {
        let sender = ScriptedSender::new();
        sender.script(WriteKind::Upload, vec![fail(cause)]);
        let rig = Rig::with(sender);
        let up = rig.hand(upload(41));
        let queued = rig.hand(create(41, 42));
        assert!(up.await.is_err());
        assert_eq!(queued.await, Err(Removal::DependsOnRemoved));
        let mut late = rig.hand(create(41, 43));
        assert_eq!(ready(&mut late), Some(Err(Removal::DependsOnRemoved)));
    }
}

#[tokio::test]
async fn given_a_delivered_upload_or_none_when_the_create_is_handed_over_then_it_is_sent() {
    let rig = Rig::new();
    let up = rig.hand(upload(41));
    let with_upload = rig.hand(create(41, 42));
    let without = rig.hand(create(51, 52));
    assert_eq!(
        (up.await, with_upload.await, without.await),
        (Ok(()), Ok(()), Ok(()))
    );
    assert_eq!(
        rig.sender.kinds(),
        vec![
            WriteKind::Started,
            WriteKind::Upload,
            WriteKind::Create,
            WriteKind::Create
        ]
    );
}

#[tokio::test]
async fn given_a_removed_answer_when_independent_writes_are_handed_over_then_each_is_delivered() {
    let sender = ScriptedSender::new();
    sender.script(WriteKind::Answer, vec![fail(refused(Operation::Result))]);
    let rig = Rig::with(sender);
    let first = rig.hand(answer());
    let independent = vec![
        saved(),
        plan(),
        step(1),
        failed_fact(),
        upload(40),
        create(41, 42),
        registration(10),
        mail(20, 99, 98),
        finish(61, 62),
        cancel(63, 64),
    ];
    assert!(first.await.is_err());
    for write in independent {
        assert_eq!(rig.hand(write.clone()).await, Ok(()), "{write:?}");
    }
}

#[tokio::test]
async fn given_a_chain_of_removals_when_the_first_is_refused_then_every_dependent_is_removed_in_turn()
 {
    let sender = ScriptedSender::new();
    sender.script(
        WriteKind::Upload,
        vec![fail(refusal(Operation::Upload, "size"))],
    );
    let rig = Rig::with(sender);
    let ends = vec![
        rig.hand(upload(41)),
        rig.hand(create(41, 42)),
        rig.hand(finish(41, 43)),
        rig.hand(cancel(41, 44)),
    ];
    let mut results = Vec::new();
    for end in ends {
        results.push(end.await);
    }
    assert!(matches!(results[0], Err(Removal::Refused(_))));
    assert!(matches!(results[1], Err(Removal::Refused(_))));
    assert_eq!(results[2], Err(Removal::DependsOnRemoved));
    assert_eq!(results[3], Err(Removal::DependsOnRemoved));
    assert_eq!(
        rig.sender.kinds(),
        vec![WriteKind::Started, WriteKind::Upload]
    );
}
