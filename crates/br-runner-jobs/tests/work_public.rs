use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_runner_jobs::test_double::{Delivery, ScriptedJobs, trigger_json, work_from_trigger};
use br_runner_jobs::{CallMode, Error, IgnoreReports, PayloadForm, QueueSettings};
use serde_json::json;
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn passport() -> Passport {
    Passport::human(
        id(5),
        false,
        true,
        AuthMethod::Jwt,
        Some(id(6)),
        PassportClaims::from_value(json!({ "department": "ocelot" })).expect("claims"),
    )
}

fn trigger() -> serde_json::Value {
    trigger_json(id(2), id(1), id(3), id(4), &passport()).expect("a trigger")
}

fn settings() -> QueueSettings {
    QueueSettings {
        start_id: id(9),
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(10),
        observer: Arc::new(IgnoreReports),
    }
}

#[tokio::test]
async fn given_a_trigger_on_the_feed_when_it_is_read_then_the_work_gives_the_wire_as_received_and_ack_settles_its_delivery()
 {
    let jobs = ScriptedJobs::new(
        br_runner_jobs::RunnerType::new("analyst").expect("a runner type"),
        "pod-1",
    );
    let mut feed = jobs
        .client()
        .triggers(Duration::from_secs(30))
        .await
        .expect("a feed");
    let mut value = trigger();
    value["attempt"] = json!(2);
    value["mode"] = json!("BACKGROUND");
    value["caller_address"] = json!(id(8));
    value["payload"] = json!({ "form": "KEPT" });
    value["added_later"] = json!({ "k": 1 });
    jobs.push_trigger(serde_json::to_vec(&value).expect("bytes"));
    let work = feed.next().await.map_err(|u| u.error).expect("a Work");
    assert_eq!(
        (work.job_id(), work.run_id(), work.attempt()),
        (id(1), id(2), 2)
    );
    assert_eq!(
        (work.parent_job_id(), work.initial_job_id()),
        (id(3), id(4))
    );
    assert_eq!(work.passport(), &passport());
    assert_eq!(work.payload_form(), Some(&PayloadForm::Kept));
    assert_eq!(
        work.call(),
        Some(CallMode::Background {
            caller_address: id(8)
        })
    );
    assert_eq!(work.unknown_fields()["added_later"], json!({ "k": 1 }));
    work.ack().await.expect("acked");
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack]);
}

#[tokio::test]
async fn given_a_trigger_without_a_passport_on_the_feed_when_read_then_it_is_terminated_and_never_a_work()
 {
    let jobs = ScriptedJobs::new(
        br_runner_jobs::RunnerType::new("analyst").expect("a runner type"),
        "pod-1",
    );
    let mut feed = jobs
        .client()
        .triggers(Duration::from_secs(30))
        .await
        .expect("a feed");
    let mut value = trigger();
    value.as_object_mut().expect("object").remove("passport");
    jobs.push_trigger(serde_json::to_vec(&value).expect("bytes"));
    let unreadable = feed.next().await.expect_err("unreadable");
    assert!(matches!(unreadable.error, Error::Unreadable(_)));
    assert_eq!(
        (unreadable.job_id, unreadable.run_id),
        (Some(id(1)), Some(id(2)))
    );
    assert_eq!(jobs.deliveries(), vec![Delivery::Terminate]);
}

#[tokio::test]
async fn given_a_work_when_the_queue_is_made_twice_then_the_second_is_invalid_input_and_giving_back_leaves_the_first_working()
 {
    let jobs = ScriptedJobs::new(
        br_runner_jobs::RunnerType::new("analyst").expect("a runner type"),
        "pod-1",
    );
    let mut feed = jobs
        .client()
        .triggers(Duration::from_secs(30))
        .await
        .expect("a feed");
    jobs.push_trigger(serde_json::to_vec(&trigger()).expect("bytes"));
    let work = feed.next().await.map_err(|u| u.error).expect("a Work");
    let (queue, started) = work.delivery_queue(settings()).await.expect("a queue");
    started.await.expect("started");
    assert!(matches!(
        work.delivery_queue(settings()).await,
        Err(Error::InvalidInput(_))
    ));
    work.give_back().await.expect("given back");
    queue.completed().await.expect("delivered after give back");
    assert_eq!(jobs.deliveries(), vec![Delivery::GiveBack]);
}

#[tokio::test]
async fn given_a_work_made_directly_from_a_trigger_value_then_it_has_no_delivery_to_settle() {
    let jobs = ScriptedJobs::new(
        br_runner_jobs::RunnerType::new("analyst").expect("a runner type"),
        "pod-1",
    );
    let work = work_from_trigger(&jobs, trigger()).expect("a Work");
    assert!(matches!(work.ack().await, Err(Error::InvalidInput(_))));
    assert!(matches!(
        work.terminate().await,
        Err(Error::InvalidInput(_))
    ));
}
