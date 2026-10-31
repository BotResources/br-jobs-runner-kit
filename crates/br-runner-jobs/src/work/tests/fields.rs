use serde_json::{Map, json};

use super::*;
use crate::model::{CallMode, Content, ContentAttachment, PayloadForm, Restart};

#[test]
fn given_a_minimal_trigger_when_read_then_every_field_of_the_wire_is_given_and_the_optional_ones_are_none()
 {
    let work = work(&jobs());
    assert_eq!(work.job_id(), id(JOB));
    assert_eq!(work.run_id(), id(RUN));
    assert_eq!(work.attempt(), 1);
    assert_eq!(work.parent_job_id(), id(PARENT));
    assert_eq!(work.initial_job_id(), id(INITIAL));
    assert_eq!(work.passport(), &passport());
    assert_eq!(work.config(), None);
    assert_eq!(work.triggered_by(), None);
    assert_eq!(work.payload_form(), None);
    assert_eq!(work.call(), None);
    assert_eq!(work.restart(), None);
    assert!(work.unknown_fields().is_empty());
}

#[test]
fn given_a_trigger_with_every_field_when_read_then_each_one_is_given_as_received() {
    let config = json!({ "model": "small", "nested": { "n": [1, 2, 3] } });
    let value = trigger_with(&[
        ("attempt", json!(4)),
        ("config", config.clone()),
        ("triggered_by", json!(id(80))),
        ("mode", json!("BACKGROUND")),
        ("caller_address", json!(id(81))),
        ("restart", json!({ "restart_of_run_id": id(82) })),
        ("payload", json!({ "form": "KEPT" })),
    ]);
    let work = work_of(&jobs(), value);
    assert_eq!(work.attempt(), 4);
    assert_eq!(work.config(), Some(&config));
    assert_eq!(work.triggered_by(), Some(id(80)));
    assert_eq!(
        work.call(),
        Some(CallMode::Background {
            caller_address: id(81)
        })
    );
    assert_eq!(
        work.restart(),
        Some(Restart {
            restart_of_run_id: id(82)
        })
    );
    assert_eq!(work.payload_form(), Some(&PayloadForm::Kept));
}

#[test]
fn given_an_inline_payload_when_read_then_the_form_holds_the_message_and_the_attachments() {
    let value = trigger_with(&[(
        "payload",
        json!({ "form": "INLINE", "message": "hello", "attachments": [
            { "name": "a.txt", "content_type": "text/plain", "content": "body" }
        ] }),
    )]);
    let work = work_of(&jobs(), value);
    assert_eq!(
        work.payload_form(),
        Some(&PayloadForm::Inline(Content {
            message: "hello".to_owned(),
            attachments: vec![ContentAttachment {
                name: "a.txt".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "body".to_owned(),
            }],
        }))
    );
}

#[test]
fn given_an_inline_payload_with_no_attachments_field_when_read_then_there_are_none() {
    let value = trigger_with(&[("payload", json!({ "form": "INLINE", "message": "m" }))]);
    let work = work_of(&jobs(), value);
    assert_eq!(
        work.payload_form(),
        Some(&PayloadForm::Inline(Content {
            message: "m".to_owned(),
            attachments: Vec::new(),
        }))
    );
}

#[test]
fn given_a_reference_payload_when_read_then_the_form_names_the_service_and_the_document() {
    let value = trigger_with(&[(
        "payload",
        json!({ "form": "REFERENCE", "service": "documents", "document_id": "doc-9" }),
    )]);
    let work = work_of(&jobs(), value);
    assert_eq!(
        work.payload_form(),
        Some(&PayloadForm::Reference {
            service: "documents".to_owned(),
            document_id: "doc-9".to_owned(),
        })
    );
}

#[test]
fn given_a_sync_call_when_read_then_the_mode_is_sync_whatever_the_caller_address_says() {
    let sync = work_of(&jobs(), trigger_with(&[("mode", json!("SYNC"))]));
    assert_eq!(sync.call(), Some(CallMode::Sync));
    let with_address = work_of(
        &jobs(),
        trigger_with(&[("mode", json!("SYNC")), ("caller_address", json!(id(81)))]),
    );
    assert_eq!(with_address.call(), Some(CallMode::Sync));
}

#[test]
fn given_a_caller_address_and_no_mode_when_read_then_there_is_no_call_mode() {
    let work = work_of(&jobs(), trigger_with(&[("caller_address", json!(id(81)))]));
    assert_eq!(work.call(), None);
}

#[test]
fn given_fields_the_library_does_not_know_when_read_then_they_are_kept_and_given_as_received() {
    let value = trigger_with(&[
        ("future_flag", json!(true)),
        (
            "future_object",
            json!({ "a": [1, null, "x"], "b": { "c": 2.5 } }),
        ),
        ("future_null", json!(null)),
    ]);
    let work = work_of(&jobs(), value);
    let mut expected = Map::new();
    expected.insert("future_flag".to_owned(), json!(true));
    expected.insert(
        "future_object".to_owned(),
        json!({ "a": [1, null, "x"], "b": { "c": 2.5 } }),
    );
    expected.insert("future_null".to_owned(), json!(null));
    assert_eq!(work.unknown_fields(), &expected);
}

#[test]
fn given_a_trigger_of_known_fields_only_when_read_then_no_known_field_leaks_into_the_unknown_ones()
{
    let value = trigger_with(&[
        ("attempt", json!(2)),
        ("config", json!({})),
        ("triggered_by", json!(id(80))),
        ("mode", json!("BACKGROUND")),
        ("caller_address", json!(id(81))),
        ("restart", json!({ "restart_of_run_id": id(82) })),
        ("payload", json!({ "form": "KEPT" })),
    ]);
    assert!(work_of(&jobs(), value).unknown_fields().is_empty());
}

#[test]
fn given_a_passport_with_claims_when_read_then_the_whole_passport_is_given() {
    let work = work(&jobs());
    assert_eq!(work.passport().user_id(), Some(id(USER)));
    assert_eq!(work.passport().impersonator_id(), Some(id(IMPERSONATOR)));
    assert_eq!(
        work.passport().claim::<String>("department"),
        Some("claim-value-ocelot".to_owned())
    );
}

#[test]
fn given_a_service_passport_when_read_then_it_is_given_as_a_service_passport() {
    let service = Passport::service(id(90), PassportClaims::new());
    let value = trigger_json(id(RUN), id(JOB), id(PARENT), id(INITIAL), &service).expect("json");
    let work = work_of(&jobs(), value);
    assert_eq!(work.passport(), &service);
    assert_eq!(work.passport().service_account_id(), Some(id(90)));
}
