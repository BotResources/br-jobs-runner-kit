#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_agentic_contract::AgentPort;
use br_graph_contract::{
    CancelToken, CancelView, Cause, CreateError, CreateFuture, Edge, Event, GraphBuilder, Instance,
    InstanceId, Node, State, Target,
};
use br_graph_executor::{Outcome, RunConfig, start};
use br_runner_toolkit::{
    FailureContext, FailureKind, FailureMapping, FailureOverride, FailureReport, Terminal,
    terminal_for,
};
use support::*;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct Boom(String);

#[derive(Clone)]
enum Behavior {
    Succeed,
    Fail(&'static str),
    Refuse(&'static str),
    Vanish,
    CouldNotStart,
    AskForAnAgent,
}

struct ToEnd;

impl Edge<Doc> for ToEnd {
    fn name(&self) -> &str {
        "writer_to_end"
    }

    fn source(&self) -> &str {
        "writer"
    }

    fn targets(&self) -> &[Target] {
        &[Target::End]
    }

    fn evaluate(&self, _state: &State<Doc>) -> Result<Vec<Target>, Cause> {
        Ok(vec![Target::End])
    }
}

struct Scripted {
    behavior: Behavior,
    rig: Arc<Rig>,
    agent_seen: Arc<Mutex<Option<br_llm_messages::AgentId>>>,
}

impl Node<Doc> for Scripted {
    fn name(&self) -> &str {
        "writer"
    }

    fn kind(&self) -> &str {
        "agentic"
    }

    fn subkind(&self) -> &str {
        "react"
    }

    fn create(&self, _cancel: CancelView, id: InstanceId) -> CreateFuture<'_, Doc> {
        let behavior = self.behavior.clone();
        let rig = Arc::clone(&self.rig);
        let seen = Arc::clone(&self.agent_seen);
        Box::pin(async move {
            if matches!(behavior, Behavior::CouldNotStart) {
                return Err(CreateError::new(Boom("no room".to_owned())));
            }
            let (instance, mut reader, sender) = Instance::<Doc>::channel(id);
            tokio::spawn(async move {
                let _ = reader.next().await;
                match behavior {
                    Behavior::Succeed => sender.finish(Event::Result(())),
                    Behavior::AskForAnAgent => {
                        let agent = rig.port.agent(&doc(), id, &label("main")).await;
                        *seen.lock().expect("seen") = agent.ok().map(|agent| agent.id);
                        sender.finish(Event::Result(()));
                    }
                    Behavior::Fail(message) => {
                        sender.finish(Event::failed(Boom(message.to_owned())))
                    }
                    Behavior::Refuse(reason) => sender.finish(Event::refused(reason)),
                    Behavior::Vanish => drop(sender),
                    Behavior::CouldNotStart => {}
                }
            });
            Ok(instance)
        })
    }
}

struct Ran {
    outcome: Outcome<Doc>,
    seen: Option<br_llm_messages::AgentId>,
    rig: Arc<Rig>,
}

async fn run(behavior: Behavior, cancel: Option<CancelToken>) -> Ran {
    let rig = Arc::new(rig());
    let seen = Arc::new(Mutex::new(None));
    let graph = GraphBuilder::<Doc>::new("1")
        .node(Arc::new(Scripted {
            behavior,
            rig: Arc::clone(&rig),
            agent_seen: Arc::clone(&seen),
        }))
        .edge(Arc::new(ToEnd))
        .entry("writer")
        .build()
        .expect("a graph");
    let mut config = RunConfig::new(cancel.unwrap_or_default(), Duration::from_millis(50));
    config.observer = Some(rig.handle.observer());
    let state = State::create(doc()).expect("a state");
    let outcome = start(Arc::new(graph), state, vec!["writer".to_owned()], config)
        .await
        .expect("started");
    let seen = *seen.lock().expect("seen");
    Ran { outcome, seen, rig }
}

fn failed(terminal: Terminal) -> FailureReport {
    match terminal {
        Terminal::Failed(report) => report,
        other => panic!("a failed run is expected, got {other:?}"),
    }
}

#[tokio::test]
async fn given_each_executor_failure_then_the_reason_code_is_its_name_and_the_kind_is_permanent() {
    let cases = [
        (Behavior::Fail("boom"), "node_failed", "boom"),
        (
            Behavior::Refuse("not twice"),
            "node_refused_an_input",
            "not twice",
        ),
        (Behavior::CouldNotStart, "node_could_not_start", "no room"),
    ];
    for (behavior, code, text) in cases {
        let ran = run(behavior, None).await;
        let report = failed(terminal_for(&ran.outcome, true, false, None));
        assert_eq!(report.reason_code, code);
        assert!(
            report.diagnostic.contains(text),
            "{code}: {}",
            report.diagnostic
        );
        assert_eq!(report.kind, FailureKind::Permanent);
        assert!(report.parameters.is_empty());
        assert_eq!(report.retry_after, None);
    }
    let vanished = run(Behavior::Vanish, None).await;
    let report = failed(terminal_for(&vanished.outcome, true, false, None));
    assert_eq!(report.reason_code, "node_vanished");
}

#[tokio::test]
async fn given_an_ended_run_then_the_state_is_handed_over_only_when_the_runner_can_restart_with_an_inbox_agent()
 {
    let ran = run(Behavior::Succeed, None).await;
    let expect = |can_restart, inbox| {
        assert_eq!(
            terminal_for(&ran.outcome, can_restart, inbox, None),
            Terminal::Completed {
                hand_over_state: can_restart && inbox
            }
        );
    };
    expect(true, true);
    expect(true, false);
    expect(false, true);
    expect(false, false);
}

#[tokio::test]
async fn given_a_cancelled_run_then_no_terminal_fact_is_due() {
    let token = CancelToken::new();
    token.handle().trigger();
    let ran = run(Behavior::Succeed, Some(token)).await;
    assert!(matches!(
        terminal_for(&ran.outcome, true, true, None),
        Terminal::Cancelled { .. }
    ));
}

#[tokio::test]
async fn given_a_failure_mapping_then_it_is_called_once_with_the_cause_and_overrides_what_it_gives()
{
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&calls);
    let mapping: FailureMapping = Arc::new(move |context: &FailureContext<'_>| {
        let boom = context
            .cause
            .and_then(|cause| cause.downcast_ref::<Boom>())
            .map(|boom| boom.0.clone());
        recorder.lock().expect("calls").push((
            context.reason_code.to_owned(),
            context.text.to_owned(),
            boom,
        ));
        FailureOverride {
            kind: Some(FailureKind::Transient),
            reason_code: Some("provider_down".to_owned()),
            retry_after: Some(Duration::from_secs(30)),
            ..FailureOverride::default()
        }
    });
    let ran = run(Behavior::Fail("boom"), None).await;
    let report = failed(terminal_for(&ran.outcome, true, false, Some(&mapping)));
    assert_eq!(report.kind, FailureKind::Transient);
    assert_eq!(report.reason_code, "provider_down");
    assert_eq!(report.diagnostic, "boom");
    assert_eq!(report.retry_after, Some(Duration::from_secs(30)));
    assert_eq!(
        *calls.lock().expect("calls"),
        vec![(
            "node_failed".to_owned(),
            "boom".to_owned(),
            Some("boom".to_owned())
        )]
    );
}

#[tokio::test]
async fn given_a_failure_mapping_that_panics_then_the_run_is_reported_with_the_reason_code_panic() {
    let mapping: FailureMapping = Arc::new(|_: &FailureContext<'_>| panic!("the mapping broke"));
    let ran = run(Behavior::Fail("boom"), None).await;
    let report = failed(terminal_for(&ran.outcome, true, false, Some(&mapping)));
    assert_eq!(report.reason_code, "panic");
    assert!(report.diagnostic.contains("the mapping broke"));
    assert_eq!(report.kind, FailureKind::Permanent);
}

#[tokio::test]
async fn given_a_node_that_asks_for_an_agent_inside_a_real_run_then_the_observer_map_answers_it() {
    let ran = run(Behavior::AskForAnAgent, None).await;
    assert!(ran.seen.is_some());
    assert_eq!(ran.rig.registered.lock().expect("registered").len(), 1);
    assert!(ran.rig.handle.has_registered_inbox_agent());
}
