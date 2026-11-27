use std::collections::BTreeMap;

use k8s_openapi::api::networking::v1::{NetworkPolicyEgressRule, NetworkPolicyIngressRule};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, LabelSelectorRequirement};

use super::*;

#[test]
fn the_default_deny_must_select_all_pods() {
    for selector in [
        LabelSelector {
            match_labels: Some(BTreeMap::from([("app".to_owned(), "x".to_owned())])),
            match_expressions: None,
        },
        LabelSelector {
            match_labels: None,
            match_expressions: Some(vec![LabelSelectorRequirement {
                key: "app".to_owned(),
                operator: "Exists".to_owned(),
                values: None,
            }]),
        },
    ] {
        let mut world = fenced_world();
        world.policies[0].spec.as_mut().unwrap().pod_selector = Some(selector);
        assert!(matches!(
            fault_of(&world).problem,
            FenceProblem::NotDefaultDeny(reason) if reason.contains("all pods")
        ));
    }
    let mut empty_maps = fenced_world();
    empty_maps.policies[0].spec.as_mut().unwrap().pod_selector = Some(LabelSelector {
        match_labels: Some(BTreeMap::new()),
        match_expressions: Some(Vec::new()),
    });
    assert!(verdict(&empty_maps).is_fenced());
}

#[test]
fn the_default_deny_types_must_hold_ingress_and_egress() {
    for types in [
        None,
        Some(vec![]),
        Some(vec!["Ingress".to_owned()]),
        Some(vec!["Egress".to_owned()]),
    ] {
        let mut world = fenced_world();
        world.policies[0].spec.as_mut().unwrap().policy_types = types;
        assert!(matches!(
            fault_of(&world).problem,
            FenceProblem::NotDefaultDeny(reason) if reason.contains("Ingress and Egress")
        ));
    }
}

#[test]
fn the_default_deny_must_have_no_allow_rule_of_either_kind() {
    let mut ingress = fenced_world();
    ingress.policies[0].spec.as_mut().unwrap().ingress =
        Some(vec![NetworkPolicyIngressRule::default()]);
    let mut egress = fenced_world();
    egress.policies[0].spec.as_mut().unwrap().egress =
        Some(vec![NetworkPolicyEgressRule::default()]);
    for world in [ingress, egress] {
        assert!(matches!(
            fault_of(&world).problem,
            FenceProblem::NotDefaultDeny(reason) if reason.contains("allow rule")
        ));
    }
    let mut empty_lists = fenced_world();
    empty_lists.policies[0].spec.as_mut().unwrap().ingress = Some(Vec::new());
    empty_lists.policies[0].spec.as_mut().unwrap().egress = Some(Vec::new());
    assert!(verdict(&empty_lists).is_fenced());
}

#[test]
fn a_default_deny_with_no_spec_is_not_a_default_deny() {
    let mut world = fenced_world();
    world.policies[0].spec = None;
    assert!(matches!(
        fault_of(&world).problem,
        FenceProblem::NotDefaultDeny(_)
    ));
}
