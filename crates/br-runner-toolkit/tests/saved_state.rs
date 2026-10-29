#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::num::NonZeroUsize;

use br_graph_contract::State;
use br_runner_toolkit::{SavedAgent, SavedState, SettingsError, ToolkitSettings};
use support::*;

fn agent() -> SavedAgent {
    SavedAgent {
        node: "writer".to_owned(),
        label: "main".to_owned(),
        agent: v7().to_string(),
        inbox: true,
    }
}

#[test]
fn given_a_state_and_agents_when_saved_and_read_back_then_both_parts_are_whole() {
    let state = State::create(doc()).expect("a state");
    let saved = SavedState::from_state(&state, vec![agent()]).expect("saved");
    let read = SavedState::from_bytes(&saved.to_bytes().expect("bytes")).expect("read");
    assert_eq!(read.agents(), [agent_like(&saved)]);
    let rebuilt: State<Doc> = read
        .rebuild(NonZeroUsize::new(1024).expect("limit"))
        .expect("rebuilt");
    assert!(rebuilt.fields() == &doc());
}

fn agent_like(saved: &SavedState) -> SavedAgent {
    saved.agents()[0].clone()
}

#[test]
fn given_a_document_of_the_first_form_when_read_by_a_later_one_then_it_is_understood() {
    let first_form = br#"{"form":1,"fields":{"note":"n"},"agents":[{"node":"writer","label":"main","agent":"01a18a02-ca54-7401-8b1f-121a3e1af401","inbox":true}]}"#;
    let read = SavedState::from_bytes(first_form).expect("read");
    assert_eq!(read.agents().len(), 1);
    assert!(read.agents()[0].inbox);
    let with_unknown_part =
        br#"{"form":2,"fields":{"note":"n"},"agents":[],"added_later":{"x":1}}"#;
    assert!(SavedState::from_bytes(with_unknown_part).is_ok());
}

#[test]
fn given_fields_over_the_limit_when_rebuilt_then_the_state_is_refused() {
    let state = State::create(doc()).expect("a state");
    let saved = SavedState::from_state(&state, Vec::new()).expect("saved");
    let rebuilt = saved.rebuild::<Doc>(NonZeroUsize::new(2).expect("limit"));
    assert!(rebuilt.is_err());
}

#[test]
fn given_garbage_when_read_then_an_error_and_never_a_default() {
    assert!(SavedState::from_bytes(b"not json").is_err());
    assert!(SavedState::from_bytes(br#"{"agents":[]}"#).is_err());
}

#[test]
fn given_fewer_than_one_concurrent_run_when_settings_are_made_then_they_are_invalid() {
    let seconds = std::time::Duration::from_secs;
    assert_eq!(
        ToolkitSettings::new(0, seconds(1), seconds(1), seconds(1), seconds(1), "1.0.0"),
        Err(SettingsError::NoSlot)
    );
    assert!(
        ToolkitSettings::new(1, seconds(1), seconds(1), seconds(1), seconds(1), "1.0.0").is_ok()
    );
}
