use contract_jobs::graphql_roots::*;
use serde_json::{Value, json};

use crate::book::tests::*;
use crate::error::{Error, Operation};
use crate::model::{ListedAgent, NewMailAttachment, NewMailInput, UnreadItem};
use crate::test_double::{Call, ScriptedJobs};

const TEXT: &str = "The plain text of jobs, for an agent.";

fn reply(root: &str, fields: Value) -> Value {
    let mut body = json!({ "success": true, "reason": null, "message": null });
    if let (Some(target), Some(extra)) = (body.as_object_mut(), fields.as_object()) {
        target.extend(extra.clone());
    }
    json!({ root: body })
}

fn refusal(root: &str) -> Value {
    json!({ root: { "success": false, "reason": "address_unknown", "message": TEXT } })
}

fn graphql_calls(jobs: &ScriptedJobs) -> Vec<(String, Value)> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Graphql {
                document,
                variables,
            } => Some((document, variables)),
            _ => None,
        })
        .collect()
}

fn mail(attachments: Vec<NewMailAttachment>) -> NewMailInput {
    NewMailInput {
        mail_id: id(420),
        from_address: id(70),
        author: "Ann".to_owned(),
        source_conversation: id(72),
        subject: "subject".to_owned(),
        body: "body".to_owned(),
        to: vec![id(71)],
        cc: vec![id(73)],
        attachments,
    }
}

#[tokio::test]
async fn given_a_mail_when_it_is_sent_through_the_handle_then_one_call_carries_the_work_the_mail_and_no_queue_is_needed()
 {
    let jobs = scripted();
    let handle = work(&jobs).mail_handle();
    handle.send_mail(mail(Vec::new())).await.expect("sent");
    let calls = graphql_calls(&jobs);
    assert_eq!(calls.len(), 1);
    let input = &calls[0].1["input"];
    assert_eq!(input["mailId"], json!(id(420)));
    assert_eq!(input["fromAddress"], json!(id(70)));
    assert_eq!(input["to"], json!([id(71)]));
    assert_eq!(input["work"]["runId"], json!(id(RUN)));
    assert!(
        jobs.calls()
            .iter()
            .all(|call| !matches!(call, Call::Fact(_)))
    );
}

#[tokio::test]
async fn given_attachments_when_the_mail_is_sent_then_each_document_goes_before_the_mail() {
    let jobs = scripted();
    let handle = work(&jobs).mail_handle();
    let attachment = NewMailAttachment {
        attachment_id: id(430),
        name: "a.txt".to_owned(),
        content_type: "text/plain".to_owned(),
        content: "QQ==".to_owned(),
    };
    handle
        .send_mail(mail(vec![attachment]))
        .await
        .expect("sent");
    let calls = graphql_calls(&jobs);
    assert_eq!(calls.len(), 2);
    assert!(calls[0].0.contains(UPLOAD_ATTACHMENT_ROOT));
    assert!(calls[1].0.contains(SEND_MAIL_ROOT));
}

#[tokio::test]
async fn given_a_refused_mail_when_it_is_sent_then_the_refusal_keeps_the_text_of_jobs() {
    let jobs = scripted();
    jobs.push_graphql(Ok(refusal(SEND_MAIL_ROOT)));
    let error = work(&jobs)
        .mail_handle()
        .send_mail(mail(Vec::new()))
        .await
        .expect_err("refused");
    assert_eq!(
        error,
        Error::refused(Operation::Mail, "address_unknown", TEXT)
    );
}

#[tokio::test]
async fn given_an_address_when_the_unread_items_are_read_then_the_call_names_that_address_and_the_order_of_jobs_is_kept()
 {
    let jobs = scripted();
    jobs.push_graphql(Ok(reply(
        UNREAD_ROOT,
        json!({ "items": [
            { "itemId": id(402), "seq": 2, "itemKind": "NOTICE", "address": id(70), "runId": id(RUN),
              "noticeType": "STARTED", "jobId": id(300) },
            { "itemId": id(401), "seq": 1, "itemKind": "CC_NOTICE", "address": id(70), "runId": id(RUN),
              "mailId": id(403), "subject": "cc" },
        ] }),
    )));
    let items = work(&jobs)
        .mail_handle()
        .unread(id(70))
        .await
        .expect("items");
    let order: Vec<_> = items.iter().map(UnreadItem::item_id).collect();
    assert_eq!(order, vec![id(402), id(401)]);
    assert_eq!(graphql_calls(&jobs)[0].1["address"], json!(id(70)));
}

#[tokio::test]
async fn given_a_mail_id_and_an_address_when_one_mail_is_read_then_the_mail_is_given_and_the_call_names_both()
 {
    let jobs = scripted();
    jobs.push_graphql(Ok(reply(
        MAIL_ROOT,
        json!({ "mail": {
            "mailId": id(403), "from": id(71), "author": "Bob", "sourceConversationId": null,
            "subject": "s", "body": "b", "to": [id(70)], "cc": [],
            "attachments": [{ "attachmentId": id(404), "name": "f", "contentType": "text/plain", "size": 3 }],
        } }),
    )));
    let read = work(&jobs)
        .mail_handle()
        .read_mail(id(403), id(70))
        .await
        .expect("mail");
    assert_eq!(read.mail_id, id(403));
    assert_eq!(read.attachments[0].attachment_id, id(404));
    let variables = &graphql_calls(&jobs)[0].1;
    assert_eq!(
        (variables["mailId"].clone(), variables["address"].clone()),
        (json!(id(403)), json!(id(70)))
    );
}

#[tokio::test]
async fn given_an_attachment_id_when_it_is_fetched_then_the_document_comes_back() {
    let jobs = scripted();
    jobs.push_graphql(Ok(reply(
        ATTACHMENT_ROOT,
        json!({ "attachment": { "name": "f", "contentType": "text/plain", "content": "QQ==" } }),
    )));
    let fetched = work(&jobs)
        .mail_handle()
        .attachment(id(403), id(404))
        .await
        .expect("attachment");
    assert_eq!(
        (fetched.name.as_str(), fetched.content.as_str()),
        ("f", "QQ==")
    );
    let variables = &graphql_calls(&jobs)[0].1;
    assert_eq!(variables["attachmentId"], json!(id(404)));
}

#[tokio::test]
async fn given_an_item_when_it_is_marked_read_then_one_call_on_the_bus_names_the_run_and_the_item()
{
    let jobs = scripted();
    work(&jobs)
        .mail_handle()
        .mark_read(id(401))
        .await
        .expect("marked");
    assert_eq!(
        jobs.calls(),
        vec![Call::MarkRead {
            run_id: id(RUN),
            item_ids: vec![id(401)]
        }]
    );
}

#[tokio::test]
async fn given_a_refused_marker_when_it_is_set_then_the_refusal_comes_back_and_is_not_repeated() {
    let jobs = scripted();
    jobs.answer_mark_read(|| Err(Error::refused(Operation::ReadMarker, "not_yours", TEXT)));
    let error = work(&jobs)
        .mail_handle()
        .mark_read(id(401))
        .await
        .expect_err("refused");
    assert!(error.is_refusal());
    assert_eq!(jobs.calls().len(), 1);
}

#[tokio::test]
async fn given_a_tree_when_the_agents_are_listed_then_the_agents_of_the_run_come_back() {
    let jobs = scripted();
    jobs.set_agents(vec![ListedAgent {
        agent_id: id(70),
        run_id: id(RUN),
        runner_type: "analyst".to_owned(),
        running: true,
        has_inbox: true,
        mail_address: Some(id(70)),
        restartable_now: None,
    }]);
    let agents = work(&jobs)
        .mail_handle()
        .list_agents()
        .await
        .expect("agents");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].mail_address, Some(id(70)));
    assert_eq!(jobs.calls(), vec![Call::ListAgents { run_id: id(RUN) }]);
}

#[tokio::test]
async fn given_a_lost_call_when_the_handle_calls_then_the_error_passes_and_the_call_is_not_repeated()
 {
    let jobs = scripted();
    jobs.push_graphql(Err(Error::lost("cut")));
    let error = work(&jobs)
        .mail_handle()
        .unread(id(70))
        .await
        .expect_err("lost");
    assert_eq!(error, Error::lost("cut"));
    assert_eq!(graphql_calls(&jobs).len(), 1);
}
