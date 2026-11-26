use br_runner_sandbox::{
    AttemptId, ClientId, SaveKey, manifest_key, part_key, record_key, records_prefix, saves_prefix,
    tombstone_key, tombstones_prefix,
};

use crate::common::{agent, thread, turn};

fn key() -> SaveKey {
    SaveKey::new(
        ClientId::new("acme").expect("client"),
        thread(),
        agent(),
        turn(),
    )
}

#[test]
fn the_keys_of_the_four_kinds_of_object_have_the_layout_of_the_store() {
    let key = key();
    let attempt = AttemptId::mint();
    let (c, t, a, n) = ("acme", key.thread(), key.agent(), key.turn());
    assert_eq!(
        part_key(&key, attempt),
        format!("saves/{c}/{t}/{a}/{n}/{attempt}/part-0.tar.zst")
    );
    assert_eq!(
        manifest_key(&key),
        format!("saves/{c}/{t}/{a}/{n}/manifest.json")
    );
    assert_eq!(record_key(&key), format!("records/{c}/{t}/{a}/{n}.json"));
    assert_eq!(
        tombstone_key(&key),
        format!("tombstones/{c}/{t}/{a}/{n}.json")
    );
}

#[test]
fn the_ids_in_a_key_are_lowercase_hyphenated_uuid_text() {
    let key = key();
    let text = manifest_key(&key);
    assert_eq!(text, text.to_lowercase());
    assert_eq!(key.thread().to_string().len(), 36);
    assert!(text.contains(&key.thread().as_uuid().hyphenated().to_string()));
}

#[test]
fn a_part_is_under_the_directory_of_its_save_and_named_by_its_attempt() {
    let key = key();
    let attempt = AttemptId::mint();
    let part = part_key(&key, attempt);
    assert!(part.starts_with(manifest_key(&key).trim_end_matches("manifest.json")));
    assert!(part.contains(&attempt.to_string()));
    assert_ne!(part, part_key(&key, AttemptId::mint()));
}

#[test]
fn two_saves_that_differ_in_one_part_of_the_key_have_no_object_in_common() {
    let base = key();
    let others = [
        SaveKey::new(
            ClientId::new("other").expect("client"),
            base.thread(),
            base.agent(),
            base.turn(),
        ),
        SaveKey::new(base.client().clone(), thread(), base.agent(), base.turn()),
        SaveKey::new(base.client().clone(), base.thread(), agent(), base.turn()),
        SaveKey::new(base.client().clone(), base.thread(), base.agent(), turn()),
    ];
    let attempt = AttemptId::mint();
    for other in others {
        assert_ne!(manifest_key(&base), manifest_key(&other));
        assert_ne!(record_key(&base), record_key(&other));
        assert_ne!(tombstone_key(&base), tombstone_key(&other));
        assert_ne!(part_key(&base, attempt), part_key(&other, attempt));
    }
}

#[test]
fn every_key_holds_the_client_the_thread_the_agent_and_the_turn() {
    let key = key();
    let keys = [
        part_key(&key, AttemptId::mint()),
        manifest_key(&key),
        record_key(&key),
        tombstone_key(&key),
    ];
    for text in keys {
        for part in [
            key.client().to_string(),
            key.thread().to_string(),
            key.agent().to_string(),
            key.turn().to_string(),
        ] {
            assert!(text.contains(&part), "{text} lacks {part}");
        }
    }
}

#[test]
fn the_prefixes_of_a_thread_cover_its_objects_and_no_other_thread() {
    let key = key();
    let client = key.client();
    let prefixes = [
        (saves_prefix(client, key.thread()), manifest_key(&key)),
        (records_prefix(client, key.thread()), record_key(&key)),
        (tombstones_prefix(client, key.thread()), tombstone_key(&key)),
    ];
    for (prefix, object) in prefixes {
        assert!(prefix.ends_with('/'));
        assert!(object.starts_with(&prefix));
        let other = SaveKey::new(client.clone(), thread(), key.agent(), key.turn());
        assert!(!manifest_key(&other).starts_with(&prefix));
        assert!(!record_key(&other).starts_with(&prefix));
        assert!(!tombstone_key(&other).starts_with(&prefix));
    }
}

#[test]
fn the_prefix_of_a_client_does_not_cover_a_client_that_starts_with_its_name() {
    let thread = thread();
    let short = ClientId::new("acme").expect("client");
    let long = ClientId::new("acme2").expect("client");
    let long_key = SaveKey::new(long, thread, agent(), turn());
    assert!(!manifest_key(&long_key).starts_with(&saves_prefix(&short, thread)));
}
