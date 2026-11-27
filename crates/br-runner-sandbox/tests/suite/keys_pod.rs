use br_runner_sandbox::{
    ANNOTATION_CLAIMED_AT, ANNOTATION_CLEAN, ANNOTATION_IDLE_SINCE, ANNOTATION_SAVED_AT,
    ANNOTATION_SEED_HASH, ANNOTATION_STATE_BYTES, ANNOTATION_STATE_TURN, CLEAN_FALSE, CLEAN_TRUE,
    CONDITION_WARM_PODS_FAILING, DEFAULT_LABEL_PREFIX, LABEL_AGENT, LABEL_MANAGED, LABEL_POOL,
    LABEL_POOL_ID, LABEL_ROLE, LABEL_THREAD, LabelPrefix, MANAGED_TRUE, POOL_CLAIMED, POOL_WARM,
    PodKeys, REASON_IMAGE_PULL_FAILING, REASON_START_CRASHING, ROLE_SANDBOX,
};

#[test]
fn the_labels_have_the_exact_text_of_the_pod_contract() {
    assert_eq!(LABEL_ROLE, "br.sandbox/role");
    assert_eq!(LABEL_POOL_ID, "br.sandbox/pool-id");
    assert_eq!(LABEL_POOL, "br.sandbox/pool");
    assert_eq!(LABEL_THREAD, "br.sandbox/thread");
    assert_eq!(LABEL_AGENT, "br.sandbox/agent");
    assert_eq!(LABEL_MANAGED, "br.sandbox/managed");
    assert_eq!(MANAGED_TRUE, "true");
}

#[test]
fn the_annotations_have_the_exact_text_of_the_pod_contract() {
    assert_eq!(ANNOTATION_CLEAN, "br.sandbox/clean");
    assert_eq!(ANNOTATION_CLAIMED_AT, "br.sandbox/claimed-at");
    assert_eq!(ANNOTATION_SEED_HASH, "br.sandbox/seed-hash");
    assert_eq!(ANNOTATION_STATE_TURN, "br.sandbox/state-turn");
    assert_eq!(ANNOTATION_SAVED_AT, "br.sandbox/saved-at");
    assert_eq!(ANNOTATION_STATE_BYTES, "br.sandbox/state-bytes");
    assert_eq!(ANNOTATION_IDLE_SINCE, "br.sandbox/idle-since");
}

#[test]
fn the_values_of_the_labels_and_annotations_are_the_ones_of_the_contract() {
    assert_eq!(ROLE_SANDBOX, "sandbox");
    assert_eq!(POOL_WARM, "warm");
    assert_eq!(POOL_CLAIMED, "claimed");
    assert_eq!(CLEAN_TRUE, "true");
    assert_eq!(CLEAN_FALSE, "false");
}

#[test]
fn the_pool_condition_and_its_reasons_have_the_exact_text_and_no_prefix() {
    assert_eq!(CONDITION_WARM_PODS_FAILING, "WarmPodsFailing");
    assert_eq!(REASON_IMAGE_PULL_FAILING, "ImagePullFailing");
    assert_eq!(REASON_START_CRASHING, "StartCrashing");
}

#[test]
fn there_is_no_key_for_a_credential_set() {
    let keys = PodKeys::default();
    let all = [
        keys.role(),
        keys.pool_id(),
        keys.pool(),
        keys.thread(),
        keys.agent(),
        keys.managed(),
        keys.clean(),
        keys.claimed_at(),
        keys.seed_hash(),
        keys.state_turn(),
        keys.saved_at(),
        keys.state_bytes(),
        keys.idle_since(),
    ];
    assert!(all.iter().all(|key| !key.contains("creds")));
}

#[test]
fn the_keys_of_the_default_prefix_are_the_public_items() {
    let keys = PodKeys::default();
    assert_eq!(DEFAULT_LABEL_PREFIX, "br.sandbox");
    assert_eq!(keys.role(), LABEL_ROLE);
    assert_eq!(keys.pool_id(), LABEL_POOL_ID);
    assert_eq!(keys.pool(), LABEL_POOL);
    assert_eq!(keys.thread(), LABEL_THREAD);
    assert_eq!(keys.agent(), LABEL_AGENT);
    assert_eq!(keys.managed(), LABEL_MANAGED);
    assert_eq!(keys.clean(), ANNOTATION_CLEAN);
    assert_eq!(keys.claimed_at(), ANNOTATION_CLAIMED_AT);
    assert_eq!(keys.seed_hash(), ANNOTATION_SEED_HASH);
    assert_eq!(keys.state_turn(), ANNOTATION_STATE_TURN);
    assert_eq!(keys.saved_at(), ANNOTATION_SAVED_AT);
    assert_eq!(keys.state_bytes(), ANNOTATION_STATE_BYTES);
    assert_eq!(keys.idle_since(), ANNOTATION_IDLE_SINCE);
}

#[test]
fn another_prefix_replaces_the_prefix_of_every_key_and_nothing_else() {
    let keys = PodKeys::new(LabelPrefix::new("acme.io").expect("prefix"));
    assert_eq!(keys.role(), "acme.io/role");
    assert_eq!(keys.pool_id(), "acme.io/pool-id");
    assert_eq!(keys.pool(), "acme.io/pool");
    assert_eq!(keys.thread(), "acme.io/thread");
    assert_eq!(keys.agent(), "acme.io/agent");
    assert_eq!(keys.managed(), "acme.io/managed");
    assert_eq!(keys.clean(), "acme.io/clean");
    assert_eq!(keys.claimed_at(), "acme.io/claimed-at");
    assert_eq!(keys.seed_hash(), "acme.io/seed-hash");
    assert_eq!(keys.state_turn(), "acme.io/state-turn");
    assert_eq!(keys.saved_at(), "acme.io/saved-at");
    assert_eq!(keys.state_bytes(), "acme.io/state-bytes");
    assert_eq!(keys.idle_since(), "acme.io/idle-since");
}

#[test]
fn the_label_prefix_of_the_configuration_gives_the_keys() {
    let config = crate::common::build(crate::common::with(
        serde_json::json!({"label_prefix": "acme.io"}),
    ))
    .expect("config");
    assert_eq!(
        PodKeys::new(config.label_prefix().clone()).pool(),
        "acme.io/pool"
    );
}
