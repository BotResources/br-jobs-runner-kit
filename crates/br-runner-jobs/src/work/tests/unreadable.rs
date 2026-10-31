use serde_json::{Value, json};

use super::*;
use crate::error::Error;
use crate::work::Unreadable;

fn refused(value: Value) -> Unreadable {
    match work_from_trigger(&jobs(), value) {
        Err(unreadable) => unreadable,
        Ok(work) => panic!("a Work was made: {work:?}"),
    }
}

fn is_unreadable(unreadable: &Unreadable) -> bool {
    matches!(unreadable.error, Error::Unreadable(_))
}

#[test]
fn given_a_trigger_with_no_passport_when_read_then_it_is_unreadable_and_carries_the_job_and_the_run()
 {
    let unreadable = refused(trigger_without("passport"));
    assert!(is_unreadable(&unreadable));
    assert_eq!(unreadable.job_id, Some(id(JOB)));
    assert_eq!(unreadable.run_id, Some(id(RUN)));
}

#[test]
fn given_a_null_passport_when_read_then_it_is_unreadable() {
    let unreadable = refused(trigger_with(&[("passport", Value::Null)]));
    assert!(is_unreadable(&unreadable));
}

#[test]
fn given_passports_that_cannot_be_decoded_when_read_then_each_is_unreadable_and_its_text_does_not_echo_it()
 {
    let undecodable = [
        json!("a-passport-as-a-string-marmot"),
        json!([1, 2, 3]),
        json!({}),
        json!({ "kind": "robot", "user_id": id(USER) }),
        json!({ "kind": "human", "user_id": id(USER) }),
        json!({ "kind": "human", "user_id": "not-a-uuid-marmot", "is_super_admin": false,
            "is_active": true, "auth_method": "jwt", "claims": {} }),
        json!({ "kind": "service", "service_account_id": id(90), "claims": [],
            "claim_marmot": "x" }),
    ];
    for passport in undecodable {
        let unreadable = refused(trigger_with(&[("passport", passport.clone())]));
        assert!(is_unreadable(&unreadable), "{passport}");
        assert_eq!(unreadable.job_id, Some(id(JOB)));
        assert_eq!(unreadable.run_id, Some(id(RUN)));
        let text = unreadable.error.to_string();
        assert!(!text.contains("marmot"), "{text}");
        assert!(!text.contains(&id(USER).to_string()), "{text}");
    }
}

#[test]
fn given_a_passport_with_a_field_the_library_does_not_know_when_read_then_it_is_unreadable_never_trimmed()
 {
    let mut passport = serde_json::to_value(passport()).expect("json");
    passport
        .as_object_mut()
        .expect("an object")
        .insert("future".to_owned(), json!(1));
    let unreadable = refused(trigger_with(&[("passport", passport)]));
    assert!(is_unreadable(&unreadable));
}

#[test]
fn given_a_background_call_with_no_caller_address_when_read_then_it_is_unreadable() {
    let unreadable = refused(trigger_with(&[("mode", json!("BACKGROUND"))]));
    assert!(is_unreadable(&unreadable));
    assert_eq!(unreadable.job_id, Some(id(JOB)));
    assert_eq!(unreadable.run_id, Some(id(RUN)));
}

#[test]
fn given_a_mode_the_wire_does_not_name_when_read_then_it_is_unreadable() {
    assert!(is_unreadable(&refused(trigger_with(&[(
        "mode",
        json!("SOMETIMES")
    )]))));
}

#[test]
fn given_a_payload_form_the_wire_does_not_name_when_read_then_it_is_unreadable() {
    assert!(is_unreadable(&refused(trigger_with(&[(
        "payload",
        json!({ "form": "TELEPATHY" })
    )]))));
}

#[test]
fn given_a_missing_required_field_when_read_then_it_is_unreadable_and_the_ids_that_can_be_read_are_carried()
 {
    for key in [
        "run_id",
        "job_id",
        "attempt",
        "parent_job_id",
        "initial_job_id",
    ] {
        let unreadable = refused(trigger_without(key));
        assert!(is_unreadable(&unreadable), "{key}");
        let expected_job = (key != "job_id").then(|| id(JOB));
        let expected_run = (key != "run_id").then(|| id(RUN));
        assert_eq!(unreadable.job_id, expected_job, "{key}");
        assert_eq!(unreadable.run_id, expected_run, "{key}");
    }
}

#[test]
fn given_ids_that_are_not_uuids_when_read_then_none_is_carried() {
    let unreadable = refused(trigger_with(&[
        ("job_id", json!("not-a-uuid")),
        ("run_id", json!(12)),
    ]));
    assert!(is_unreadable(&unreadable));
    assert_eq!((unreadable.job_id, unreadable.run_id), (None, None));
}

#[test]
fn given_values_that_are_not_objects_when_read_then_each_is_unreadable_with_no_ids() {
    for value in [
        json!(null),
        json!(5),
        json!("trigger"),
        json!([]),
        json!([1, 2, 3, 4, 5, 6, 7]),
    ] {
        let unreadable = refused(value.clone());
        assert!(is_unreadable(&unreadable), "{value}");
        assert_eq!((unreadable.job_id, unreadable.run_id), (None, None));
    }
}

#[test]
fn given_an_attempt_that_is_not_a_number_when_read_then_it_is_unreadable() {
    assert!(is_unreadable(&refused(trigger_with(&[(
        "attempt",
        json!("one")
    )]))));
    assert!(is_unreadable(&refused(trigger_with(&[(
        "attempt",
        json!(-1)
    )]))));
}
