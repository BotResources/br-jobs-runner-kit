use super::*;

#[test]
fn a_scale_down_step_below_one_is_a_fault() {
    assert_eq!(
        load(&with("scale_down_step", json!(0))),
        Err(ConfigError::StepBelowOne)
    );
    assert!(load(&with("scale_down_step", json!(1))).is_ok());
}

#[test]
fn the_lease_durations_must_fall_in_strict_order() {
    for (duration, renew, retry) in [(10, 10, 2), (15, 10, 10), (5, 10, 2), (15, 2, 10)] {
        let mut base = with("lease_duration_seconds", json!(duration));
        base["renew_deadline_seconds"] = json!(renew);
        base["retry_period_seconds"] = json!(retry);
        assert_eq!(
            load(&base),
            Err(ConfigError::LeaseOrder),
            "{duration} {renew} {retry}"
        );
    }
}

#[test]
fn the_time_zone_must_be_a_known_name() {
    assert!(load(&with("timezone", json!("Europe/Paris"))).is_ok());
    assert_eq!(
        load(&with("timezone", json!("Mars/Olympus"))),
        Err(ConfigError::UnknownTimezone("Mars/Olympus".to_owned()))
    );
}

#[test]
fn the_store_endpoint_is_an_http_url_with_no_user_information() {
    assert!(
        load(&with(
            "store_endpoint",
            json!("http://object-store.test:8333")
        ))
        .is_ok()
    );
    for bad in [
        "s3.example.test",
        "ftp://host",
        "https://user:pw@host",
        "file:///x",
    ] {
        assert!(
            matches!(
                load(&with("store_endpoint", json!(bad))),
                Err(ConfigError::Invalid { .. })
            ),
            "{bad}"
        );
    }
}

#[test]
fn the_listen_address_is_a_socket_address() {
    assert!(matches!(
        load(&with("listen_address", json!("localhost"))),
        Err(ConfigError::Invalid { .. })
    ));
}

#[test]
fn a_blank_text_value_is_a_fault() {
    assert!(matches!(
        load(&with("store_bucket", json!("  "))),
        Err(ConfigError::Invalid { .. })
    ));
}

#[test]
fn a_document_that_holds_the_key_pools_is_a_fault_because_pools_are_resources() {
    for value in [json!([]), json!([{"id": "p"}]), json!(null)] {
        let error = load(&with("pools", value)).unwrap_err();
        assert_eq!(error, ConfigError::PoolsAreResources);
        assert!(error.to_string().contains("resources"), "{error}");
    }
}

#[test]
fn the_agent_container_has_no_default_and_is_not_blank() {
    assert_eq!(
        load(&without("agent_container")),
        Err(ConfigError::Missing {
            key: "agent_container".to_owned()
        })
    );
    for blank in ["", "  "] {
        assert!(matches!(
            load(&with("agent_container", json!(blank))),
            Err(ConfigError::Invalid { key, .. }) if key == "agent_container"
        ));
    }
    assert!(load(&with("agent_container", json!(3))).is_err());
}

#[test]
fn the_keys_that_moved_to_the_resource_are_faults_in_the_document() {
    for key in [
        "min_replicas",
        "max_replicas",
        "refill_seconds",
        "margin",
        "floors",
        "image",
        "size",
    ] {
        let result = load(&with(key, json!(1)));
        assert!(
            matches!(&result, Err(ConfigError::Document(text)) if text.contains(key)),
            "{key}: {result:?}"
        );
    }
}
