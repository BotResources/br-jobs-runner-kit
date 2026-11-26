use std::time::Duration;

use br_runner_sandbox::{
    AttemptId, ClientId, FormatError, Manifest, PartReport, SaveKey, Sha256Hex, Timestamp, part_key,
};
use serde_json::{Value, json};

use crate::common::{agent, thread, turn};

fn report() -> PartReport {
    PartReport {
        entries: 7,
        bytes_raw: 4096,
        bytes_stored: 1024,
        sha256_raw: Sha256Hex::of(b"raw"),
        sha256_stored: Sha256Hex::of(b"stored"),
    }
}

fn save_key() -> SaveKey {
    SaveKey::new(
        ClientId::new("acme").expect("client"),
        thread(),
        agent(),
        turn(),
    )
}

fn made(
    key: &SaveKey,
    parent: Option<br_agentic_harness_contract::TurnId>,
) -> (Manifest, AttemptId) {
    let attempt = AttemptId::mint();
    let manifest = Manifest::new(
        key.clone(),
        parent,
        attempt,
        Timestamp::now(),
        vec!["/work".to_owned(), "/home/agent".to_owned()],
        report(),
    );
    (manifest, attempt)
}

fn json_of(manifest: &Manifest) -> Value {
    serde_json::from_slice(&manifest.to_vec().expect("bytes")).expect("json")
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("bytes")
}

#[test]
fn the_manifest_has_exactly_the_fields_of_the_layout() {
    let key = save_key();
    let (manifest, attempt) = made(&key, Some(turn()));
    let value = json_of(&manifest);
    let mut fields: Vec<&str> = value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        vec![
            "agent",
            "attempt",
            "client",
            "compression",
            "entries",
            "made_at",
            "parent",
            "parts",
            "schema",
            "scope",
            "tar_format",
            "thread",
            "turn",
        ]
    );
    assert_eq!(value["schema"], 1);
    assert_eq!(value["tar_format"], "pax");
    assert_eq!(value["compression"], "zstd");
    assert_eq!(value["client"], "acme");
    assert_eq!(value["thread"], key.thread().to_string());
    assert_eq!(value["agent"], key.agent().to_string());
    assert_eq!(value["turn"], key.turn().to_string());
    assert_eq!(value["attempt"], attempt.to_string());
    assert_eq!(value["entries"], 7);
    assert_eq!(value["scope"], json!(["/work", "/home/agent"]));
}

#[test]
fn the_manifest_names_one_part_with_the_fields_of_the_layout() {
    let key = save_key();
    let (manifest, attempt) = made(&key, None);
    let part = &json_of(&manifest)["parts"];
    assert_eq!(part.as_array().expect("list").len(), 1);
    let part = &part[0];
    let mut fields: Vec<&str> = part
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        vec![
            "bytes_raw",
            "bytes_stored",
            "index",
            "key",
            "sha256_raw",
            "sha256_stored"
        ]
    );
    assert_eq!(part["index"], 0);
    assert_eq!(part["key"], part_key(&key, attempt));
    assert_eq!(part["bytes_raw"], 4096);
    assert_eq!(part["bytes_stored"], 1024);
    assert_eq!(part["sha256_raw"], Sha256Hex::of(b"raw").to_string());
    assert_eq!(part["sha256_stored"], Sha256Hex::of(b"stored").to_string());
    assert_eq!(part["sha256_raw"].as_str().expect("text").len(), 64);
}

#[test]
fn the_parent_is_the_parent_turn_text_or_null() {
    let key = save_key();
    let parent = turn();
    assert_eq!(
        json_of(&made(&key, Some(parent)).0)["parent"],
        parent.to_string()
    );
    assert_eq!(json_of(&made(&key, None).0)["parent"], Value::Null);
}

#[test]
fn a_manifest_read_back_is_the_manifest_that_was_written() {
    let key = save_key();
    let (manifest, _) = made(&key, Some(turn()));
    let read = Manifest::parse_for(&manifest.to_vec().expect("bytes"), &key).expect("read");
    assert_eq!(read, manifest);
    assert_eq!(read.entries(), 7);
    assert_eq!(read.part().bytes_raw, 4096);
}

#[test]
fn the_receipt_of_a_manifest_holds_the_turn_the_time_and_the_raw_bytes() {
    let key = save_key();
    let (manifest, _) = made(&key, None);
    let receipt = manifest.receipt();
    assert_eq!(receipt.turn, key.turn());
    assert_eq!(receipt.made_at, manifest.made_at().to_system_time());
    assert_eq!(receipt.bytes, 4096);
}

#[test]
fn a_manifest_of_another_thread_agent_turn_or_client_is_not_read_for_a_key() {
    let key = save_key();
    let (manifest, _) = made(&key, None);
    let bytes = manifest.to_vec().expect("bytes");
    let others = [
        SaveKey::new(
            ClientId::new("other").expect("client"),
            key.thread(),
            key.agent(),
            key.turn(),
        ),
        SaveKey::new(key.client().clone(), thread(), key.agent(), key.turn()),
        SaveKey::new(key.client().clone(), key.thread(), agent(), key.turn()),
        SaveKey::new(key.client().clone(), key.thread(), key.agent(), turn()),
    ];
    for other in others {
        assert_eq!(
            Manifest::parse_for(&bytes, &other),
            Err(FormatError::OtherKey { object: "manifest" })
        );
    }
}

fn corrupted(change: impl FnOnce(&mut Value)) -> Result<Manifest, FormatError> {
    let (manifest, _) = made(&save_key(), None);
    let mut value = json_of(&manifest);
    change(&mut value);
    Manifest::parse(&bytes(&value))
}

#[test]
fn a_manifest_that_breaks_a_rule_of_the_layout_fails_to_load() {
    assert!(corrupted(|v| v["schema"] = json!(2)).is_err());
    assert!(corrupted(|v| v["tar_format"] = json!("gnu")).is_err());
    assert!(corrupted(|v| v["compression"] = json!("gzip")).is_err());
    assert!(corrupted(|v| v["parts"] = json!([])).is_err());
    assert!(corrupted(|v| v["parts"][0]["index"] = json!(1)).is_err());
    assert!(corrupted(|v| v["parts"][0]["key"] = json!("saves/elsewhere/part-0.tar.zst")).is_err());
    assert!(corrupted(|v| v["parts"][0]["sha256_raw"] = json!("ABC")).is_err());
    assert!(corrupted(|v| v["parts"][0]["sha256_stored"] = json!("A".repeat(64))).is_err());
    assert!(corrupted(|v| v["thread"] = json!("nope")).is_err());
    assert!(corrupted(|v| v["agent"] = json!("nope")).is_err());
    assert!(corrupted(|v| v["turn"] = json!("nope")).is_err());
    assert!(corrupted(|v| v["parent"] = json!("nope")).is_err());
    assert!(corrupted(|v| v["attempt"] = json!("nope")).is_err());
    assert!(corrupted(|v| v["made_at"] = json!("yesterday")).is_err());
    assert!(corrupted(|v| v["client"] = json!("a/b")).is_err());
    assert!(corrupted(|v| v["entries"] = json!(-1)).is_err());
    assert!(
        corrupted(|v| {
            v.as_object_mut().map(|map| map.remove("parts"));
        })
        .is_err()
    );
}

#[test]
fn a_part_key_that_is_another_attempt_than_the_manifest_attempt_is_refused() {
    let key = save_key();
    let foreign = part_key(&key, AttemptId::mint());
    assert!(corrupted(|v| v["parts"][0]["key"] = json!(foreign)).is_err());
}

#[test]
fn a_manifest_that_is_not_json_is_a_format_error() {
    assert!(matches!(
        Manifest::parse(b"{"),
        Err(FormatError::Malformed {
            object: "manifest",
            ..
        })
    ));
}

#[test]
fn the_time_of_the_manifest_is_the_time_of_the_receipt_and_gives_one_expiry() {
    let (manifest, _) = made(&save_key(), None);
    let read = Manifest::parse(&manifest.to_vec().expect("bytes")).expect("read");
    let ttl = Duration::from_secs(3600);
    assert_eq!(read.made_at().plus(ttl), manifest.made_at().plus(ttl));
    assert_eq!(read.receipt().made_at, manifest.receipt().made_at);
}

#[test]
fn a_manifest_written_by_another_producer_with_the_same_fields_is_read() {
    let key = save_key();
    let attempt = AttemptId::mint();
    let literal = json!({
        "schema": 1,
        "client": "acme",
        "thread": key.thread().to_string(),
        "agent": key.agent().to_string(),
        "turn": key.turn().to_string(),
        "parent": null,
        "attempt": attempt.to_string(),
        "made_at": "2026-11-26T10:00:00Z",
        "scope": ["/work"],
        "tar_format": "pax",
        "compression": "zstd",
        "entries": 3,
        "parts": [{
            "index": 0,
            "key": part_key(&key, attempt),
            "bytes_raw": 10,
            "bytes_stored": 5,
            "sha256_raw": "a".repeat(64),
            "sha256_stored": "b".repeat(64),
        }],
    });
    let manifest = Manifest::parse_for(&bytes(&literal), &key).expect("read");
    assert_eq!(manifest.made_at().to_string(), "2026-11-26T10:00:00.000Z");
    assert_eq!(manifest.scope(), ["/work".to_owned()]);
}
