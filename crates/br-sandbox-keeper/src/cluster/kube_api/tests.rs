use std::time::Duration;

mod reads;
mod rights;
mod stub;
mod watch_failures;
mod wire;

use super::lease::{lease_facts, lease_object};
use super::watch::pod_selector;
use super::*;
use crate::time::at;

#[test]
fn a_lease_goes_through_the_object_and_back() {
    let write = LeaseWrite {
        holder: "keeper-a".to_owned(),
        acquire_time: at(100),
        renew_time: at(200),
        duration: Duration::from_secs(15),
        transitions: 3,
    };
    let lease = lease_object("br-sandbox-keeper", "sys", &write, Some("42"));
    let facts = lease_facts(&lease);
    assert_eq!(facts.holder.as_deref(), Some("keeper-a"));
    assert_eq!(facts.renew_time, Some(at(200)));
    assert_eq!(facts.duration, Some(Duration::from_secs(15)));
    assert_eq!(facts.transitions, 3);
    assert_eq!(facts.resource_version, "42");
}

#[test]
fn a_lease_with_an_empty_holder_has_no_holder() {
    let mut lease = lease_object(
        "n",
        "s",
        &LeaseWrite {
            holder: "x".to_owned(),
            acquire_time: at(1),
            renew_time: at(1),
            duration: Duration::from_secs(1),
            transitions: 0,
        },
        None,
    );
    lease.spec.as_mut().unwrap().holder_identity = Some(String::new());
    assert_eq!(lease_facts(&lease).holder, None);
}

#[test]
fn the_pod_selector_names_the_two_pool_states_under_the_prefix() {
    assert_eq!(
        pod_selector(&PodKeys::default()),
        "br.sandbox/pool in (warm,claimed)"
    );
    let other = PodKeys::new(br_runner_sandbox::LabelPrefix::new("acme.sbx").unwrap());
    assert_eq!(pod_selector(&other), "acme.sbx/pool in (warm,claimed)");
}
