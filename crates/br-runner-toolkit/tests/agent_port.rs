#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::AgentPort;
use br_graph_executor::Observation;
use br_runner_toolkit::{PortErrorCode, PortSetRequest, PortSets, SavedAgent, SavedState};
use support::*;

fn code_of(fault: &br_agentic_contract::PortFault) -> PortErrorCode {
    fault
        .cause()
        .downcast_ref::<br_runner_toolkit::PortError>()
        .expect("a port error")
        .code()
}

#[tokio::test]
async fn given_a_port_set_not_started_when_an_agent_is_asked_then_the_call_is_a_port_error() {
    let sets = PortSets::new(None);
    let set = sets.port_set::<Doc>(
        "writer",
        PortSetRequest::new().author(label("main"), author("W")),
    );
    let fault = set
        .agent
        .agent(&doc(), instance(), &label("main"))
        .await
        .expect_err("not started");
    assert_eq!(code_of(&fault), PortErrorCode::NotStarted);
}

#[tokio::test]
async fn given_one_key_when_asked_twice_then_one_uuidv7_identity_is_registered_once() {
    let rig = rig();
    let first = rig.create("writer");
    let second = rig.create("writer");
    let a = rig.agent(first, "main").await;
    let b = rig.agent(second, "main").await;
    assert_eq!(a.id, b.id);
    assert_eq!(a.id.as_uuid().get_version_num(), 7);
    assert_eq!(a.author, Some(author("Writer")));
    assert_eq!(rig.registered.lock().expect("registered").len(), 1);
}

#[tokio::test]
async fn given_another_label_when_asked_then_another_identity_without_an_inbox() {
    let rig = rig();
    let at = rig.create("writer");
    let main = rig.agent(at, "main").await;
    let helper = rig.agent(at, "helper").await;
    assert_ne!(main.id, helper.id);
    let registered = rig.registered.lock().expect("registered").clone();
    let of = |id| {
        registered
            .iter()
            .find(|r| r.agent == id)
            .copied()
            .expect("registered")
    };
    assert!(of(main.id).has_inbox);
    assert!(of(main.id).can_restart);
    assert!(!of(helper.id).has_inbox);
    assert!(!of(helper.id).can_restart);
}

#[tokio::test]
async fn given_a_runner_that_cannot_restart_when_the_inbox_agent_registers_then_it_says_so() {
    let rig = rig_with(Some(false), None);
    let at = rig.create("writer");
    rig.agent(at, "main").await;
    let registered = rig.registered.lock().expect("registered").clone();
    assert!(registered[0].has_inbox);
    assert!(!registered[0].can_restart);
}

#[tokio::test]
async fn given_concurrent_calls_for_one_key_then_they_share_one_registration() {
    let rig = Arc::new(rig());
    let at = rig.create("writer");
    let calls = (0..8).map(|_| {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move { rig.agent(at, "main").await.id })
    });
    let ids = futures_join(calls).await;
    assert!(ids.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(rig.registered.lock().expect("registered").len(), 1);
}

async fn futures_join(
    calls: impl Iterator<Item = tokio::task::JoinHandle<br_llm_messages::AgentId>>,
) -> Vec<br_llm_messages::AgentId> {
    let mut ids = Vec::new();
    for call in calls {
        ids.push(call.await.expect("joined"));
    }
    ids
}

#[tokio::test]
async fn given_a_refused_registration_when_asked_then_a_port_error_and_the_key_is_forgotten() {
    let rig = rig_with(None, Some(refusing_registrar()));
    let at = rig.create("writer");
    let fault = rig
        .port
        .agent(&doc(), at, &label("main"))
        .await
        .expect_err("refused");
    assert_eq!(code_of(&fault), PortErrorCode::JobsRefused);
    let again = rig
        .port
        .agent(&doc(), at, &label("main"))
        .await
        .expect_err("refused again");
    assert_eq!(code_of(&again), PortErrorCode::JobsRefused);
}

#[tokio::test]
async fn given_a_label_without_an_author_name_when_asked_then_a_port_error() {
    let rig = rig();
    let at = rig.create("writer");
    let fault = rig
        .port
        .agent(&doc(), at, &label("stranger"))
        .await
        .expect_err("no name");
    assert_eq!(code_of(&fault), PortErrorCode::NoAuthorName);
    assert!(rig.registered.lock().expect("registered").is_empty());
}

#[tokio::test(start_paused = true)]
async fn given_an_instance_not_yet_told_when_asked_then_the_call_waits_for_the_observer() {
    let rig = Arc::new(rig());
    let at = instance();
    let asking = {
        let rig = Arc::clone(&rig);
        tokio::spawn(async move { rig.agent(at, "main").await.id })
    };
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert!(!asking.is_finished());
    rig.handle.observer().observe(Observation::InstanceCreated {
        instance: at,
        node: "writer".to_owned(),
        kind: "agentic".to_owned(),
        subkind: String::new(),
    });
    asking.await.expect("answered");
}

#[tokio::test]
async fn given_a_restart_when_the_map_is_loaded_then_saved_identities_and_flags_are_kept() {
    let rig = rig();
    let saved_id = v7();
    let state = SavedState::from_bytes(
        serde_json::to_vec(&serde_json::json!({
            "form": 1,
            "fields": { "note": "n" },
            "agents": [ { "node": "writer", "label": "main", "agent": saved_id.to_string(), "inbox": true } ]
        }))
        .expect("json")
        .as_slice(),
    )
    .expect("saved state");
    let handle = rig
        .sets
        .begin_run(v7(), Some(&state))
        .expect("restarted run");
    let at = instance();
    handle.observer().observe(Observation::InstanceCreated {
        instance: at,
        node: "writer".to_owned(),
        kind: "agentic".to_owned(),
        subkind: String::new(),
    });
    let agent = rig.agent(at, "main").await;
    assert_eq!(agent.id.as_uuid(), saved_id);
    assert!(rig.registered.lock().expect("registered").is_empty());
    assert_eq!(
        handle.agents(),
        vec![SavedAgent {
            node: "writer".to_owned(),
            label: "main".to_owned(),
            agent: saved_id.to_string(),
            inbox: true
        }]
    );
}

#[tokio::test]
async fn given_a_run_that_ended_when_its_agent_is_used_then_the_identity_is_forgotten() {
    let rig = rig();
    let agent = rig.live_agent().await;
    assert_eq!(rig.handle.agents().len(), 1);
    rig.handle.end();
    let thread = rig.thread;
    let fault = br_agentic_contract::ThreadAccess::load(rig.access.as_ref(), &thread, agent)
        .await
        .expect_err("run ended");
    assert_eq!(code_of(&fault), PortErrorCode::UnknownAgent);
}
