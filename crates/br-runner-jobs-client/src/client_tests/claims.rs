use std::time::Duration;

use contract_jobs::runner::ClaimRefusal;
use serde_json::json;

use super::*;
use crate::error::{Error, Operation};
use crate::model::CallMode;
use crate::test_double::{Call, Delivery};

const WAIT: Duration = Duration::from_secs(1);

fn claims(jobs: &ScriptedJobs) -> Vec<Uuid> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Claim { claim_id } => Some(claim_id),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn given_a_zero_wait_or_a_nil_id_when_claiming_then_invalid_input_and_nothing_is_sent() {
    let (jobs, client) = scripted();
    assert!(matches!(
        client.claim(id(10), Duration::ZERO).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        client.claim(Uuid::nil(), WAIT).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(claims(&jobs).is_empty());
}

#[tokio::test]
async fn given_a_run_in_the_reply_when_claiming_then_it_is_a_work_with_the_claim_id_sent_and_no_trigger()
 {
    let (jobs, client) = scripted();
    jobs.push_claim_reply(Ok(reply(Some(trigger()))));
    let work = client
        .claim(id(10), WAIT)
        .await
        .expect("a reply")
        .expect("a Work");
    assert_eq!(claims(&jobs), vec![id(10)]);
    assert_eq!((work.job_id(), work.run_id()), (id(1), id(2)));
    assert!(matches!(work.ack().await, Err(Error::InvalidInput(_))));
    assert!(matches!(
        work.give_back().await,
        Err(Error::InvalidInput(_))
    ));
}

#[tokio::test]
async fn given_a_work_from_a_claim_when_it_is_terminated_then_invalid_input_and_nothing_is_settled()
{
    let (jobs, client) = scripted();
    jobs.push_claim_reply(Ok(reply(Some(trigger()))));
    let work = client.claim(id(10), WAIT).await.expect("ok").expect("Work");
    assert!(matches!(
        work.terminate().await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(jobs.deliveries(), Vec::<Delivery>::new());
}

#[tokio::test]
async fn given_a_run_with_every_field_when_claiming_then_the_work_gives_them_as_received_and_keeps_the_unknown_ones()
 {
    let (jobs, client) = scripted();
    let mut run = trigger();
    run["attempt"] = json!(3);
    run["config"] = json!({ "k": [1, 2] });
    run["triggered_by"] = json!(id(80));
    run["mode"] = json!("BACKGROUND");
    run["caller_address"] = json!(id(81));
    run["restart"] = json!({ "restart_of_run_id": id(82) });
    run["payload"] = json!({ "form": "KEPT" });
    run["future_field"] = json!({ "x": 1 });
    jobs.push_claim_reply(Ok(reply(Some(run))));
    let work = client.claim(id(10), WAIT).await.expect("ok").expect("Work");
    assert_eq!(work.attempt(), 3);
    assert_eq!(work.config(), Some(&json!({ "k": [1, 2] })));
    assert_eq!(work.triggered_by(), Some(id(80)));
    assert_eq!(
        work.call(),
        Some(CallMode::Background {
            caller_address: id(81)
        })
    );
    assert_eq!(work.restart().map(|r| r.restart_of_run_id), Some(id(82)));
    assert_eq!(work.unknown_fields().len(), 1);
    assert_eq!(work.unknown_fields()["future_field"], json!({ "x": 1 }));
}

#[tokio::test]
async fn given_an_empty_reply_when_claiming_then_none() {
    let (jobs, client) = scripted();
    for body in [
        br"{}".to_vec(),
        reply(None),
        br#"{"version":2,"run":null}"#.to_vec(),
    ] {
        jobs.push_claim_reply(Ok(body));
        assert!(client.claim(id(10), WAIT).await.expect("ok").is_none());
    }
}

#[tokio::test]
async fn given_a_refusal_when_claiming_then_refused_names_the_reason_as_the_wire_names_it() {
    let (jobs, client) = scripted();
    for reason in ClaimRefusal::ALL {
        jobs.push_claim_reply(Ok(serde_json::to_vec(
            &json!({ "version": 2, "reason": reason.as_str() }),
        )
        .expect("json")));
        match client.claim(id(10), WAIT).await {
            Err(Error::Refused(refusal)) => {
                assert_eq!(refusal.operation, Operation::Claim);
                assert_eq!(refusal.reason, reason.as_str());
                assert_eq!(refusal.retry_after_ms, None);
            }
            other => panic!("{reason:?}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn given_a_busy_refusal_when_claiming_then_the_delay_of_jobs_is_carried() {
    let (jobs, client) = scripted();
    jobs.push_claim_reply(Ok(
        br#"{"version":2,"reason":"busy","retry_after_ms":1500}"#.to_vec(),
    ));
    match client.claim(id(10), WAIT).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.reason, ClaimRefusal::Busy.as_str());
            assert_eq!(refusal.retry_after_ms, Some(1500));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn given_a_reason_the_library_does_not_know_when_claiming_then_the_word_is_kept_and_it_is_refused()
 {
    let (jobs, client) = scripted();
    jobs.push_claim_reply(Ok(
        br#"{"version":2,"reason":"brand_new_reason","retry_after_ms":9}"#.to_vec(),
    ));
    match client.claim(id(10), WAIT).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.reason, "brand_new_reason");
            assert_eq!(refusal.retry_after_ms, None);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn given_a_reason_and_a_run_together_when_claiming_then_the_refusal_wins_and_no_work_is_made()
{
    let (jobs, client) = scripted();
    let mut value = json!({ "version": 2, "reason": "claim_spent" });
    value["run"] = trigger();
    jobs.push_claim_reply(Ok(serde_json::to_vec(&value).expect("json")));
    assert!(matches!(
        client.claim(id(10), WAIT).await,
        Err(Error::Refused(_))
    ));
}

#[tokio::test]
async fn given_a_reply_that_cannot_be_decoded_when_claiming_then_unreadable_and_never_a_work() {
    let (jobs, client) = scripted();
    let mut no_passport = trigger();
    no_passport
        .as_object_mut()
        .expect("object")
        .remove("passport");
    let mut bad_passport = trigger();
    bad_passport["passport"] = json!({ "kind": "robot", "marker": "secret-marmot" });
    let mut background = trigger();
    background["mode"] = json!("BACKGROUND");
    let bodies = [
        b"".to_vec(),
        b"{".to_vec(),
        b"[]".to_vec(),
        b"null".to_vec(),
        b"5".to_vec(),
        br#""text""#.to_vec(),
        br#"{"reason":5}"#.to_vec(),
        br#"{"version":2,"run":5}"#.to_vec(),
        br#"{"version":2,"run":[]}"#.to_vec(),
        reply(Some(no_passport)),
        reply(Some(bad_passport)),
        reply(Some(background)),
    ];
    for body in bodies {
        let shown = String::from_utf8_lossy(&body).into_owned();
        jobs.push_claim_reply(Ok(body));
        match client.claim(id(10), WAIT).await {
            Err(Error::Unreadable(text)) => assert!(!text.contains("marmot"), "{shown}: {text}"),
            other => panic!("{shown}: {:?}", other.map(|_| ())),
        }
    }
}

#[tokio::test]
async fn given_a_transport_error_when_claiming_then_it_is_passed_on_and_the_claim_is_not_sent_again()
 {
    let (jobs, client) = scripted();
    jobs.push_claim_reply(Err(Error::wait_fired("no reply")));
    assert!(matches!(
        client.claim(id(10), WAIT).await,
        Err(Error::Lost(_))
    ));
    assert_eq!(claims(&jobs), vec![id(10)]);
}
