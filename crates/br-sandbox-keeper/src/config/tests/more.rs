use super::*;

#[test]
fn min_replicas_above_max_replicas_is_a_fault() {
    let mut base = with_pool("min_replicas", json!(21));
    base["pools"][0]["max_replicas"] = json!(20);
    assert_eq!(
        load(&base),
        Err(ConfigError::MinAboveMax {
            pool: "acme-none".to_owned()
        })
    );
    let mut base = with_pool("min_replicas", json!(20));
    base["pools"][0]["max_replicas"] = json!(20);
    assert!(load(&base).is_ok());
}

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
fn a_floor_window_that_ends_no_later_than_it_begins_is_a_fault() {
    for (from, to) in [("09:00", "09:00"), ("22:00", "02:00"), ("10:00", "09:59")] {
        let result = load(&with_pool(
            "floors",
            json!([{"from": from, "to": to, "floor": 5}]),
        ));
        assert_eq!(
            result,
            Err(ConfigError::WindowEndsNoLater {
                pool: "acme-none".to_owned(),
                index: 0
            }),
            "{from} {to}"
        );
    }
}

#[test]
fn a_window_that_ends_at_midnight_ends_at_24_00() {
    let config = load(&with_pool(
        "floors",
        json!([{"from": "22:00", "to": "24:00", "floor": 5}, {"from": "00:00", "to": "02:00", "floor": 5}]),
    ))
    .unwrap();
    assert_eq!(config.pools[0].floors[0].to_second, 86_400);
    assert_eq!(config.pools[0].floors[1].from_second, 0);
}

#[test]
fn a_time_of_day_that_is_not_a_time_is_a_fault() {
    for text in ["9:00", "25:00", "10:61", "10", "ten", "24:00:01"] {
        let result = load(&with_pool(
            "floors",
            json!([{"from": text, "to": "24:00", "floor": 5}]),
        ));
        assert!(matches!(result, Err(ConfigError::Invalid { .. })), "{text}");
    }
    let result = load(&with_pool(
        "floors",
        json!([{"from": "24:00", "to": "24:00", "floor": 5}]),
    ));
    assert!(matches!(result, Err(ConfigError::Invalid { .. })));
}

#[test]
fn two_pools_with_one_id_or_one_replica_set_in_one_namespace_are_a_fault() {
    let mut base = document();
    let mut second = base["pools"][0].clone();
    second["replicaset"] = json!("warm-other");
    base["pools"].as_array_mut().unwrap().push(second);
    assert_eq!(
        load(&base),
        Err(ConfigError::DuplicatePoolId("acme-none".to_owned()))
    );
    let mut base = document();
    let mut second = base["pools"][0].clone();
    second["id"] = json!("other");
    base["pools"].as_array_mut().unwrap().push(second);
    assert_eq!(
        load(&base),
        Err(ConfigError::DuplicateReplicaSet {
            namespace: "sbx-acme-none".to_owned(),
            replicaset: "warm-acme-none".to_owned()
        })
    );
    let mut second = base["pools"][1].clone();
    second["namespace"] = json!("sbx-other");
    base["pools"][1] = second;
    assert!(load(&base).is_ok());
}

#[test]
fn a_document_with_no_pool_is_a_fault() {
    assert_eq!(load(&with("pools", json!([]))), Err(ConfigError::NoPools));
}

#[test]
fn the_store_endpoint_is_an_http_url_with_no_user_information() {
    assert!(load(&with("store_endpoint", json!("http://seaweed:8333"))).is_ok());
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
fn the_pool_lookup_needs_the_namespace_and_the_id() {
    let config = load(&document()).unwrap();
    assert!(config.pool("sbx-acme-none", "acme-none").is_some());
    assert!(config.pool("sbx-other", "acme-none").is_none());
    assert!(config.pool("sbx-acme-none", "other").is_none());
}

#[test]
fn a_key_that_the_library_does_not_know_is_a_fault_at_every_level_of_the_document() {
    let floor = json!([{"from": "08:00", "to": "09:00", "floor": 1, "colour": "red"}]);
    let result = load(&with_pool("floors", floor));
    assert!(matches!(result, Err(ConfigError::Document(text)) if text.contains("colour")));
    let known = json!([{"from": "08:00", "to": "09:00", "floor": 1}]);
    assert!(load(&with_pool("floors", known)).is_ok());
    let result = load(&with_pool("margin_seconds", json!(1)));
    assert!(matches!(result, Err(ConfigError::Document(text)) if text.contains("margin_seconds")));
}
