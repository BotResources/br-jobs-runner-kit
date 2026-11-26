use br_runner_sandbox::{
    AttemptId, FormatError, Record, RecordError, RecordState, TOMBSTONE_REASON, Timestamp,
    Tombstone,
};
use serde_json::{Value, json};
use std::time::Duration;

fn at(seconds: u64) -> Timestamp {
    Timestamp::from_system_time(std::time::UNIX_EPOCH + Duration::from_secs(seconds))
}

fn value(record: &Record) -> Value {
    serde_json::from_slice(&record.to_vec().expect("bytes")).expect("json")
}

#[test]
fn a_started_record_has_no_end_time_and_no_error() {
    let record = Record::started(AttemptId::mint(), at(100));
    let json = value(&record);
    assert_eq!(json["state"], "started");
    assert_eq!(json["started_at"], "1970-01-01T00:01:40.000Z");
    assert!(json.get("ended_at").is_none());
    assert!(json.get("error").is_none());
}

#[test]
fn a_done_record_has_an_end_time_and_no_error() {
    let record = Record::started(AttemptId::mint(), at(100)).done(at(130));
    let json = value(&record);
    assert_eq!(json["state"], "done");
    assert_eq!(json["ended_at"], "1970-01-01T00:02:10.000Z");
    assert!(json.get("error").is_none());
}

#[test]
fn a_failed_record_has_an_end_time_and_one_of_the_three_errors() {
    for (error, text) in [
        (RecordError::Exec, "exec"),
        (RecordError::Store, "store"),
        (RecordError::Timeout, "timeout"),
    ] {
        let record = Record::started(AttemptId::mint(), at(100)).failed(at(110), error);
        let json = value(&record);
        assert_eq!(json["state"], "failed");
        assert_eq!(json["error"], text);
        assert_eq!(json["ended_at"], "1970-01-01T00:01:50.000Z");
    }
}

#[test]
fn a_record_has_the_fields_of_the_layout_and_no_file_name_or_content() {
    let record = Record::started(AttemptId::mint(), at(1)).failed(at(2), RecordError::Exec);
    let mut fields: Vec<_> = value(&record)
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        vec!["attempt", "ended_at", "error", "started_at", "state"]
    );
}

#[test]
fn a_record_read_back_is_the_record_that_was_written() {
    let attempt = AttemptId::mint();
    for record in [
        Record::started(attempt, at(1)),
        Record::started(attempt, at(1)).done(at(5)),
        Record::started(attempt, at(1)).failed(at(5), RecordError::Store),
    ] {
        assert_eq!(Record::parse(&record.to_vec().expect("bytes")), Ok(record));
    }
}

#[test]
fn the_age_of_a_record_is_the_end_time_when_present_else_the_start_time() {
    let started = Record::started(AttemptId::mint(), at(100));
    assert_eq!(started.age_anchor(), at(100));
    assert_eq!(started.done(at(140)).age_anchor(), at(140));
    assert_eq!(
        started.failed(at(150), RecordError::Exec).age_anchor(),
        at(150)
    );
}

#[test]
fn a_record_with_a_state_that_does_not_fit_its_fields_fails_to_load() {
    let attempt = AttemptId::mint().to_string();
    let cases = [
        json!({"state": "started", "attempt": attempt, "started_at": "2026-01-01T00:00:00Z", "ended_at": "2026-01-01T00:00:01Z"}),
        json!({"state": "done", "attempt": attempt, "started_at": "2026-01-01T00:00:00Z"}),
        json!({"state": "done", "attempt": attempt, "started_at": "2026-01-01T00:00:00Z", "ended_at": "2026-01-01T00:00:01Z", "error": "exec"}),
        json!({"state": "failed", "attempt": attempt, "started_at": "2026-01-01T00:00:00Z", "ended_at": "2026-01-01T00:00:01Z"}),
        json!({"state": "failed", "attempt": attempt, "started_at": "2026-01-01T00:00:00Z", "error": "exec"}),
        json!({"state": "failed", "attempt": attempt, "started_at": "2026-01-01T00:00:00Z", "ended_at": "2026-01-01T00:00:01Z", "error": "disk"}),
        json!({"state": "paused", "attempt": attempt, "started_at": "2026-01-01T00:00:00Z"}),
        json!({"state": "started", "attempt": "nope", "started_at": "2026-01-01T00:00:00Z"}),
        json!({"state": "started", "attempt": attempt}),
    ];
    for case in cases {
        let bytes = serde_json::to_vec(&case).expect("bytes");
        assert!(Record::parse(&bytes).is_err(), "{case}");
    }
}

#[test]
fn a_record_state_is_matched_by_its_variants() {
    let record = Record::started(AttemptId::mint(), at(1)).done(at(2));
    assert_eq!(record.state(), RecordState::Done { ended_at: at(2) });
    assert_eq!(record.started_at(), at(1));
}

#[test]
fn a_tombstone_is_read_for_its_existence_and_its_expiry_time() {
    let tombstone = Tombstone::new("turn-id", at(10), at(20));
    assert_eq!(tombstone.reason(), TOMBSTONE_REASON);
    assert_eq!(TOMBSTONE_REASON, "expired");
    let read = Tombstone::parse(&tombstone.to_vec().expect("bytes")).expect("read");
    assert_eq!(read.expired_at(), at(20));
    assert_eq!(read.made_at(), at(10));
    assert_eq!(read.turn(), "turn-id");
}

#[test]
fn a_tombstone_has_the_fields_of_the_layout() {
    let json: Value =
        serde_json::from_slice(&Tombstone::new("t", at(10), at(20)).to_vec().expect("bytes"))
            .expect("json");
    let mut fields: Vec<_> = json.as_object().expect("object").keys().cloned().collect();
    fields.sort_unstable();
    assert_eq!(fields, vec!["expired_at", "made_at", "reason", "turn"]);
}

#[test]
fn a_tombstone_that_is_not_valid_fails_to_load() {
    assert!(matches!(
        Tombstone::parse(b"{\"turn\": \"t\"}"),
        Err(FormatError::Malformed {
            object: "tombstone",
            ..
        })
    ));
}
