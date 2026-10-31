use contract_jobs::graphql_roots::PAYLOAD_ROOT;

use super::text_forms::{assert_clean, assert_clean_except_ids};
use super::*;
use crate::error::{CredentialFailure, CredentialKind, Error, Operation};
use crate::work::Unreadable;

fn assert_error_clean(label: &str, error: &Error) {
    assert_clean(&format!("{label} debug"), &format!("{error:?}"));
    assert_clean(&format!("{label} display"), &error.to_string());
}

#[tokio::test]
async fn given_every_failure_of_the_graphql_layer_when_a_work_reads_its_payload_then_no_error_text_shows_the_passport_the_token_or_the_job_ids()
 {
    let jobs = jobs();
    let work = work(&jobs);
    let scripted = [
        Err(Error::Unauthenticated),
        Err(Error::InfraMissing("the bucket".to_owned())),
        Err(Error::Credential(CredentialFailure {
            kind: CredentialKind::NoUsable,
            text: "the credential gave no token".to_owned(),
        })),
        Err(Error::Credential(CredentialFailure {
            kind: CredentialKind::FetchFailed,
            text: "the fetch failed".to_owned(),
        })),
        Err(Error::unavailable("connection refused")),
        Err(Error::lost("cut body")),
        Err(Error::wait_fired("late")),
        Ok(json!({ PAYLOAD_ROOT: { "success": false, "reason": "gone",
            "message": "the text of jobs" } })),
        Ok(json!({ "unrelated": 1 })),
        Ok(json!({ PAYLOAD_ROOT: { "success": true } })),
        Ok(json!({ PAYLOAD_ROOT: { "success": false } })),
        Ok(json!(null)),
    ];
    let mut produced = Vec::new();
    for answer in scripted {
        jobs.push_graphql(answer);
        produced.push(work.payload().await.expect_err("an error"));
    }
    produced.push(work.payload().await.expect_err("no scripted answer"));
    let kinds = produced
        .iter()
        .map(std::mem::discriminant)
        .collect::<std::collections::HashSet<_>>();
    assert!(kinds.len() >= 7, "{produced:?}");
    for error in &produced {
        assert_error_clean("a payload error", error);
    }
    match &produced[7] {
        Error::Refused(refusal) => {
            assert_eq!(refusal.operation, Operation::Payload);
            assert_eq!(refusal.message, "the text of jobs");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn given_the_invalid_input_paths_when_the_calls_fail_then_no_error_text_shows_the_passport_or_the_job_ids()
 {
    let jobs = jobs();
    let work = work(&jobs);
    let (queue, _) = work.delivery_queue(settings()).await.expect("queue");
    work.agent_book(&queue, Vec::new(), false).expect("book");
    let errors = [
        work.delivery_queue(settings()).await.expect_err("twice"),
        work.agent_book(&queue, Vec::new(), false)
            .expect_err("twice"),
        jobs.client()
            .triggers(Duration::ZERO)
            .await
            .expect_err("zero"),
        jobs.client()
            .claim(Uuid::nil(), Duration::from_secs(1))
            .await
            .expect_err("nil"),
        jobs.client()
            .claim(id(10), Duration::ZERO)
            .await
            .expect_err("zero"),
        work.ack().await.expect_err("claim work"),
    ];
    for error in &errors[..5] {
        assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
    }
    for error in &errors {
        assert_error_clean("an input error", error);
    }
}

#[tokio::test]
async fn given_a_work_from_a_claim_when_it_settles_then_the_refusal_names_no_id() {
    let jobs = jobs();
    let client = jobs.client();
    jobs.push_claim_reply(Ok(serde_json::to_vec(
        &json!({ "version": 2, "run": trigger() }),
    )
    .expect("bytes")));
    let work = client
        .claim(id(10), Duration::from_secs(1))
        .await
        .expect("ok")
        .expect("Work");
    assert_error_clean("ack", &work.ack().await.expect_err("no trigger"));
    assert_error_clean(
        "terminate",
        &work.terminate().await.expect_err("no trigger"),
    );
}

#[tokio::test]
async fn given_unreadable_triggers_and_claims_when_shown_then_only_the_job_and_the_run_of_the_event_appear_and_never_the_passport()
 {
    let jobs = jobs();
    let client = jobs.client();
    let mut bad_passport = trigger();
    bad_passport["passport"] = serde_json::to_value(passport()).expect("json");
    bad_passport["passport"]["future_marker"] = json!(1);
    let mut bad_mode = trigger();
    bad_mode["mode"] = json!("BACKGROUND");
    for value in [bad_passport, bad_mode, trigger_without("attempt")] {
        let unreadable = work_from_trigger(&jobs, value.clone()).expect_err("unreadable");
        let Unreadable { error, .. } = &unreadable;
        assert_error_clean("an unreadable error", error);
        assert_clean_except_ids(
            "an unreadable event",
            &format!("{unreadable:?}"),
            &["job id", "job id simple"],
        );
        jobs.push_claim_reply(Ok(serde_json::to_vec(
            &json!({ "version": 2, "run": value }),
        )
        .expect("bytes")));
        let claimed = client
            .claim(id(10), Duration::from_secs(1))
            .await
            .expect_err("unreadable");
        assert_error_clean("a claim error", &claimed);
    }
}

#[tokio::test]
async fn given_a_book_dropped_when_an_end_wait_ends_then_the_error_text_is_clean() {
    let jobs = jobs();
    let work = work(&jobs);
    let (queue, _) = work.delivery_queue(settings()).await.expect("queue");
    let book = work.agent_book(&queue, Vec::new(), false).expect("book");
    let waiting = book.caller_handle().end_wait(id(300));
    drop(book);
    let error = waiting.await.expect_err("the book was dropped");
    assert_eq!(error, Error::BookDropped);
    assert_error_clean("book dropped", &error);
}
