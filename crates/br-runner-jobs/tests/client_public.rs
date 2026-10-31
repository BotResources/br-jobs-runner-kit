use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_runner_jobs::test_double::{ScriptedJobs, trigger_json};
use br_runner_jobs::{
    CancelEvent, Capacity, DispatchMode, Error, ModeEvent, Operation, PresenceStatus,
};
use serde_json::json;
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn trigger() -> serde_json::Value {
    let passport = Passport::human(
        id(5),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::new(),
    );
    trigger_json(id(2), id(1), id(3), id(4), &passport).expect("a trigger")
}

#[tokio::test]
async fn given_a_claim_reply_with_a_run_when_claimed_then_a_work_without_trigger_and_a_busy_reply_names_its_delay()
 {
    let jobs = ScriptedJobs::new("analyst", "pod-1");
    let client = jobs.client();
    let mut run = trigger();
    run["added_later"] = json!(true);
    jobs.push_claim_reply(Ok(
        serde_json::to_vec(&json!({ "version": 2, "run": run })).expect("bytes")
    ));
    let work = client
        .claim(id(10), Duration::from_secs(1))
        .await
        .expect("a reply")
        .expect("a Work");
    assert_eq!(work.unknown_fields()["added_later"], json!(true));
    assert!(matches!(work.ack().await, Err(Error::InvalidInput(_))));
    jobs.push_claim_reply(Ok(
        br#"{"version":2,"reason":"busy","retry_after_ms":250}"#.to_vec()
    ));
    match client.claim(id(11), Duration::from_secs(1)).await {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.operation, Operation::Claim);
            assert_eq!(refusal.reason, "busy");
            assert_eq!(refusal.retry_after_ms, Some(250));
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
    jobs.push_claim_reply(Ok(b"[]".to_vec()));
    assert!(matches!(
        client.claim(id(12), Duration::from_secs(1)).await,
        Err(Error::Unreadable(_))
    ));
}

#[tokio::test]
async fn given_scripted_feeds_when_read_then_an_unreadable_event_goes_on_and_a_loss_ends_the_feed()
{
    let jobs = ScriptedJobs::new("analyst", "pod-1");
    let client = jobs.client();
    let mut modes = client.dispatch_modes().await.expect("modes");
    let mut cancels = client.cancel_orders().await.expect("cancels");
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Claim)));
    jobs.push_mode(Err(Error::Unreadable("an entry".to_owned())));
    jobs.push_mode(Ok(ModeEvent::NoEntry));
    assert_eq!(modes.next().await, Ok(ModeEvent::Mode(DispatchMode::Claim)));
    assert!(matches!(modes.next().await, Err(Error::Unreadable(_))));
    assert_eq!(modes.next().await, Ok(ModeEvent::NoEntry));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
    jobs.push_cancel(Err(Error::InfraMissing("the bucket".to_owned())));
    assert_eq!(cancels.next().await, Ok(CancelEvent::ReplayComplete));
    assert!(matches!(cancels.next().await, Err(Error::InfraMissing(_))));
    assert!(matches!(cancels.next().await, Err(Error::InfraMissing(_))));
}

#[tokio::test]
async fn given_a_presence_when_published_then_the_capacity_below_one_cannot_be_made() {
    let jobs = ScriptedJobs::new("analyst", "pod-1");
    let client = jobs.client();
    assert!(matches!(Capacity::new(0), Err(Error::InvalidInput(_))));
    client
        .publish_presence(
            "0.1.0",
            PresenceStatus::Draining,
            Capacity::new(2).expect("capacity"),
        )
        .await
        .expect("published");
    let record = &jobs.presence_records()[0];
    assert_eq!(
        (
            record.runner_version.as_str(),
            record.status,
            record.capacity
        ),
        ("0.1.0", PresenceStatus::Draining, 2)
    );
}
