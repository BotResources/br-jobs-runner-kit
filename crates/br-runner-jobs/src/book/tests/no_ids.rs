use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;
use crate::arrival::Arrival;
use crate::book::AgentKey;
use crate::caller::CreateChildInput;
use crate::error::Error;
use crate::model::{
    CallMode, ListedAgent, NewMailInput, PayloadForm, ResultRef, SyncEnd, UnreadItem,
};
use crate::queue::{QueueObserver, QueueReport, QueueSettings};
use crate::test_double::ScriptedJobs;

pub(super) const TEXT: &str = "The plain text of jobs, for an agent.";

#[derive(Default)]
pub(super) struct Recorder(pub(super) Mutex<Vec<String>>);

impl QueueObserver for Recorder {
    fn report(&self, report: &QueueReport) {
        self.0
            .lock()
            .expect("a recorder")
            .push(format!("{report:?}"));
    }
}

fn refused(root: &str) -> Value {
    json!({ root: { "success": false, "reason": "run_ended", "message": TEXT } })
}

fn ok_with(root: &str, fields: Value) -> Value {
    let mut body = json!({ "success": true, "reason": null, "message": null });
    if let (Some(target), Some(extra)) = (body.as_object_mut(), fields.as_object()) {
        target.extend(extra.clone());
    }
    json!({ root: body })
}

fn mail_item() -> Value {
    json!({
        "itemId": id(401), "seq": 1, "itemKind": "MAIL", "address": id(70), "runId": id(RUN),
        "mailId": id(402), "subject": "subject", "from": id(71), "author": "Ann",
        "sourceConversationId": id(72), "body": "body", "attachments": [],
    })
}

pub(super) fn run_notice(kind: &str, extra: Value) -> Value {
    let mut item = json!({
        "itemId": id(410), "seq": 2, "itemKind": "NOTICE", "address": null, "runId": id(RUN),
        "noticeType": kind, "jobId": id(300),
    });
    if let (Some(target), Some(more)) = (item.as_object_mut(), extra.as_object()) {
        target.extend(more.clone());
    }
    item
}

fn forbidden() -> Vec<String> {
    [JOB, PARENT, INITIAL, USER, IMPERSONATOR]
        .into_iter()
        .flat_map(|n| [id(n).to_string(), id(n).simple().to_string()])
        .collect()
}

pub(super) fn assert_clean(corpus: &[String]) {
    let all = corpus.join("\n");
    assert!(all.len() > 500, "the corpus is too small to prove anything");
    assert!(
        all.contains(&id(300).to_string()) && all.contains(&id(RUN).to_string()),
        "the allowed ids must show up, or the search proves nothing"
    );
    for needle in forbidden() {
        assert!(!all.contains(&needle), "{needle} leaked in:\n{all}");
    }
}

fn mail_input() -> NewMailInput {
    NewMailInput {
        mail_id: id(420),
        from_address: id(70),
        author: "Ann".to_owned(),
        source_conversation: id(72),
        subject: "s".to_owned(),
        body: "b".to_owned(),
        to: vec![id(71)],
        cc: Vec::new(),
        attachments: Vec::new(),
    }
}

#[tokio::test]
async fn given_a_work_with_known_ids_and_passport_when_both_handles_and_the_book_are_used_then_no_value_error_or_text_form_holds_them()
 {
    let jobs: Arc<ScriptedJobs> = scripted();
    let recorder = Arc::new(Recorder::default());
    let work = work(&jobs);
    let (queue, _) = work
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
    let mut corpus = vec![
        format!("{book:?} {book}"),
        format!("{caller:?} {caller}"),
        format!("{mail:?} {mail}"),
        format!("{:?}", book.agents()),
    ];

    let child = id(300);
    let mut input = CreateChildInput::new(
        child,
        id(301),
        crate::RunnerType::new("analyst").expect("a runner type"),
        CallMode::Sync,
        PayloadForm::Kept,
    );
    input.config = Some(json!({ "k": "v" }));
    let ends = [
        caller.upload_payload(child, content("p")).expect("upload"),
        caller.create_child(input.clone()).expect("create"),
        caller.finish(child, id(302)).expect("finish"),
        caller.cancel(child, id(303)).expect("cancel"),
    ];
    corpus.push(format!("{input:?}"));
    for end in ends {
        corpus.push(format!("{:?}", end.await));
    }
    queue.drained().await;
    corpus.push(format!("{:?}", queue.state()));

    jobs.push_graphql(Ok(refused(contract_jobs::graphql_roots::READ_RESULT_ROOT)));
    let read = caller.read_result(ResultRef(id(310)), true).await;
    corpus.push(format!("{read:?} {}", read.as_ref().expect_err("refused")));

    jobs.push_graphql(Err(Error::lost("cut")));
    let lost = mail.send_mail(mail_input()).await.expect_err("lost");
    corpus.push(format!("{lost:?} {lost}"));
    corpus.push(format!("{:?}", mail.send_mail(mail_input()).await));

    jobs.push_graphql(Ok(ok_with(
        contract_jobs::graphql_roots::UNREAD_ROOT,
        json!({ "items": [mail_item()] }),
    )));
    let unread: Vec<UnreadItem> = mail.unread(id(70)).await.expect("unread");
    corpus.push(format!("{unread:?}"));
    jobs.push_graphql(Ok(ok_with(
        contract_jobs::graphql_roots::MAIL_ROOT,
        json!({ "mail": {
            "mailId": id(402), "from": id(71), "author": "Ann", "sourceConversationId": id(72),
            "subject": "s", "body": "b", "to": [id(70)], "cc": [], "attachments": [],
        } }),
    )));
    corpus.push(format!("{:?}", mail.read_mail(id(402), id(70)).await));
    jobs.push_graphql(Ok(ok_with(
        contract_jobs::graphql_roots::ATTACHMENT_ROOT,
        json!({ "attachment": { "name": "n", "contentType": "text/plain", "content": "QQ==" } }),
    )));
    corpus.push(format!("{:?}", mail.attachment(id(402), id(403)).await));
    jobs.push_graphql(Ok(refused(contract_jobs::graphql_roots::MAIL_ROOT)));
    corpus.push(format!("{:?}", mail.read_mail(id(402), id(70)).await));
    jobs.set_agents(vec![ListedAgent {
        agent_id: id(70),
        run_id: id(RUN),
        runner_type: crate::RunnerType::new("analyst").expect("a runner type"),
        running: true,
        has_inbox: true,
        mail_address: Some(id(70)),
        restartable_now: Some(false),
    }]);
    corpus.push(format!("{:?}", mail.list_agents().await));
    corpus.push(format!("{:?}", mail.mark_read(id(401)).await));

    jobs.push_graphql(Ok(ok_with(
        contract_jobs::graphql_roots::UNREAD_ROOT,
        json!({ "items": [
            mail_item(),
            run_notice("RUN_COMPLETED", json!({ "resultId": id(311) })),
        ] }),
    )));
    let mut feed = work.arrival_feed().await.expect("a feed");
    corpus.push(format!("{feed:?}"));
    for _ in 0..3 {
        let arrival = feed.next().await.expect("an arrival");
        corpus.push(format!("{arrival:?}"));
        book.arrival(&arrival);
        if let Arrival::SyncEnd { child_job_id, .. } = &arrival {
            let end = caller.end_wait(*child_job_id);
            corpus.push(format!("{end:?}"));
            let outcome = end.await;
            assert!(matches!(outcome, Ok(SyncEnd::Completed { .. })));
            corpus.push(format!("{outcome:?}"));
        }
    }
    for error in [
        Error::BookDropped,
        Error::InfraMissing("the stream".to_owned()),
        Error::Unreadable("an end".to_owned()),
    ] {
        corpus.push(format!("{error:?} {error}"));
    }
    let report_lines = recorder.0.lock().expect("a recorder").clone();
    corpus.extend(report_lines);
    assert_clean(&corpus);
}
