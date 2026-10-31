#[path = "../examples/run_one_work.rs"]
#[allow(dead_code)]
mod run_one_work;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_runner_jobs::test_double::{Call, Delivery, ScriptedJobs, trigger_json};
use contract_jobs::graphql_roots::{PAYLOAD_ROOT, SAVE_STATE_ROOT, UPLOAD_RESULT_ROOT};
use serde_json::{Value, json};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn success() -> Value {
    json!({ "success": true, "reason": null, "message": null })
}

fn passport() -> Passport {
    Passport::human(
        id(5),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::new(),
    )
}

#[tokio::test]
async fn the_work_of_run_one_work_ends_with_the_answer_the_saved_state_and_the_completed_fact() {
    let jobs = ScriptedJobs::new("analyst", "pod-1");
    let mut payload = success();
    payload["content"] = json!({ "message": "the tickets", "attachments": [] });
    jobs.push_graphql(Ok(json!({ PAYLOAD_ROOT: payload })));
    jobs.graphql_default(json!({ UPLOAD_RESULT_ROOT: success(), SAVE_STATE_ROOT: success() }));
    let mut feed = jobs
        .client()
        .triggers(std::time::Duration::from_secs(30))
        .await
        .expect("a feed");
    let trigger = trigger_json(id(2), id(1), id(3), id(4), &passport()).expect("a trigger");
    jobs.push_trigger(serde_json::to_vec(&trigger).expect("bytes"));
    let work = feed.next().await.map_err(|u| u.error).expect("a Work");

    let report = run_one_work::run_one_work(work)
        .await
        .expect("a finished Work");

    assert_eq!(report, "0 writes left");
    let calls = jobs.calls();
    let graphql_documents = calls
        .iter()
        .filter_map(|call| match call {
            Call::Graphql { document, .. } => Some(document.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(graphql_documents[0].contains(PAYLOAD_ROOT));
    assert!(graphql_documents[1].contains(UPLOAD_RESULT_ROOT));
    assert!(graphql_documents[2].contains(SAVE_STATE_ROOT));
    assert_eq!(graphql_documents.len(), 3);
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack]);
    let uploaded = calls
        .iter()
        .find_map(|call| match call {
            Call::Graphql {
                variables,
                document,
            } if document.contains(UPLOAD_RESULT_ROOT) => Some(variables.to_string()),
            _ => None,
        })
        .expect("the answer upload");
    assert!(uploaded.contains("received: the tickets"), "{uploaded}");
}
