use std::sync::Arc;

use serde_json::{Value, json};

use super::no_ids::{Recorder, TEXT, assert_clean, run_notice};
use super::*;
use crate::arrival::{Arrival, settings_of_tests};
use crate::book::AgentKey;
use crate::error::{Error, Operation};
use crate::model::SyncEnd;
use crate::queue::QueueSettings;
use crate::reason::RefusalReason;

fn unread_reply(items: Vec<Value>) -> Value {
    json!({ UNREAD_ROOT: { "success": true, "reason": null, "message": null, "items": items } })
}

fn mail_notice() -> Value {
    json!({
        "itemId": id(420), "seq": 3, "itemKind": "MAIL_NOTICE", "address": id(70),
        "runId": id(RUN), "mailId": id(421), "noticeType": "REFUSED", "message": TEXT,
    })
}

fn call_notice(kind: &str) -> Value {
    run_notice(kind, json!({ "address": id(70), "message": TEXT }))
}

#[tokio::test]
async fn given_a_refused_started_fact_and_every_notice_and_a_cancelled_sync_end_then_no_text_form_or_error_holds_an_id()
 {
    let jobs: Arc<_> = scripted();
    jobs.answer_started(|| {
        Err(Error::refused(
            Operation::StartedFact,
            RefusalReason::RunHeld,
            TEXT,
        ))
    });
    let recorder = Arc::new(Recorder::default());
    let work = work(&jobs);
    let (queue, started) = work
        .delivery_queue(QueueSettings {
            observer: recorder.clone(),
            ..settings()
        })
        .await
        .expect("a queue");
    let restored = RestoredAgent {
        key: AgentKey::new("node", "writer"),
        agent_id: id(70),
        has_inbox: true,
    };
    let book = work
        .agent_book(&queue, vec![restored], true)
        .expect("a book");
    let caller = book.caller_handle();
    let mail = work.mail_handle();
    let mut corpus = vec![format!("{book:?}"), format!("{started:?}")];
    corpus.push(format!("{:?}", started.await));
    queue.drained().await;
    corpus.push(format!("{:?}", queue.state()));

    jobs.push_graphql(Ok(unread_reply(vec![
        mail_notice(),
        call_notice("JOB_CANCELLED"),
        call_notice("REFUSED"),
    ])));
    let unread = mail.unread(id(70)).await.expect("unread");
    corpus.push(format!("{unread:?}"));
    jobs.push_graphql(Ok(unread_reply(vec![
        mail_notice(),
        run_notice("JOB_CANCELLED", json!({ "message": TEXT })),
        run_notice("REFUSED", json!({ "message": TEXT })),
    ])));
    let mut feed = work
        .arrival_feed(settings_of_tests())
        .await
        .expect("a feed");
    let mut cancelled = false;
    for _ in 0..4 {
        let arrival = feed.next().await.expect("an arrival");
        corpus.push(format!("{arrival:?}"));
        book.arrival(&arrival);
        if let Arrival::SyncEnd { child_job_id, .. } = &arrival {
            let outcome = caller.end_wait(*child_job_id).await;
            corpus.push(format!("{outcome:?}"));
            cancelled |= matches!(outcome, Ok(SyncEnd::Cancelled { .. }));
        }
    }
    assert!(cancelled, "a cancelled sync end must be in the corpus");
    corpus.extend(recorder.0.lock().expect("a recorder").clone());
    assert_clean(&corpus);
}
