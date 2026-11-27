use k8s_openapi::api::admissionregistration::v1::{
    MatchResources, ValidatingAdmissionPolicyBindingSpec,
};
use k8s_openapi::api::networking::v1::{
    NetworkPolicyEgressRule, NetworkPolicyIngressRule, NetworkPolicySpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelectorRequirement, ObjectMeta};

use super::*;
use crate::rights::{NETWORK_POLICY_EGRESS_ALLOW, NETWORK_POLICY_EGRESS_FULL};

const SELECTOR: &str = "br.sandbox/managed=true";

struct World {
    labels: BTreeMap<String, String>,
    quotas: Vec<ResourceQuota>,
    policies: Vec<NetworkPolicy>,
    binding: Option<ValidatingAdmissionPolicyBinding>,
}

fn named<T: Default>(name: &str, build: impl FnOnce(ObjectMeta) -> T) -> T {
    build(ObjectMeta {
        name: Some(name.to_owned()),
        ..ObjectMeta::default()
    })
}

fn deny_all() -> NetworkPolicy {
    named(NETWORK_POLICY_DEFAULT_DENY, |metadata| NetworkPolicy {
        metadata,
        spec: Some(NetworkPolicySpec {
            pod_selector: Some(LabelSelector::default()),
            policy_types: Some(vec!["Ingress".to_owned(), "Egress".to_owned()]),
            ..NetworkPolicySpec::default()
        }),
    })
}

fn policy(name: &str) -> NetworkPolicy {
    named(name, |metadata| NetworkPolicy {
        metadata,
        spec: Some(NetworkPolicySpec::default()),
    })
}

fn binding(selector: Option<LabelSelector>) -> ValidatingAdmissionPolicyBinding {
    named(ADMISSION_BINDING_NAME, |metadata| ValidatingAdmissionPolicyBinding {
        metadata,
        spec: Some(ValidatingAdmissionPolicyBindingSpec {
            match_resources: Some(MatchResources {
                namespace_selector: selector,
                ..MatchResources::default()
            }),
            ..ValidatingAdmissionPolicyBindingSpec::default()
        }),
    })
}

fn fenced_world() -> World {
    World {
        labels: BTreeMap::from([("br.sandbox/managed".to_owned(), "true".to_owned())]),
        quotas: vec![named(QUOTA_NAME, |metadata| ResourceQuota {
            metadata,
            ..ResourceQuota::default()
        })],
        policies: vec![deny_all()],
        binding: Some(binding(Some(LabelSelector {
            match_labels: Some(BTreeMap::from([(
                "br.sandbox/managed".to_owned(),
                "true".to_owned(),
            )])),
            match_expressions: None,
        }))),
    }
}

fn verdict(world: &World) -> FenceVerdict {
    fence_verdict(&FenceFacts::new(
        "sbx-acme",
        &world.labels,
        SELECTOR,
        &world.quotas,
        &world.policies,
        world.binding.as_ref(),
    ))
}

fn fault_of(world: &World) -> FenceFault {
    match verdict(world) {
        FenceVerdict::NotFenced(fault) => fault,
        FenceVerdict::Fenced => panic!("the namespace is fenced"),
    }
}

#[test]
fn a_namespace_with_the_four_objects_is_fenced() {
    assert!(verdict(&fenced_world()).is_fenced());
}

#[test]
fn a_namespace_that_does_not_match_the_selector_is_not_fenced_and_the_namespace_is_named() {
    let mut world = fenced_world();
    world.labels.clear();
    let fault = fault_of(&world);
    assert_eq!(fault.object, FenceObject::Namespace);
    assert_eq!(fault.name, "sbx-acme");
    assert!(fault.to_string().contains("sbx-acme"));
    world.labels.insert("br.sandbox/managed".to_owned(), "false".to_owned());
    assert_eq!(fault_of(&world).problem, FenceProblem::NotSelected);
}

#[test]
fn a_selector_that_cannot_be_read_is_never_a_fence() {
    let world = fenced_world();
    let result = fence_verdict(&FenceFacts::new(
        "sbx-acme",
        &world.labels,
        "a in (",
        &world.quotas,
        &world.policies,
        world.binding.as_ref(),
    ));
    assert!(matches!(
        result,
        FenceVerdict::NotFenced(FenceFault { problem: FenceProblem::SelectorUnreadable(_), .. })
    ));
}

#[test]
fn a_missing_quota_is_named_and_a_quota_of_another_name_is_no_quota() {
    let mut world = fenced_world();
    world.quotas.clear();
    let fault = fault_of(&world);
    assert_eq!((fault.object, fault.name.as_str()), (FenceObject::ResourceQuota, "br-sandbox-quota"));
    assert_eq!(fault.problem, FenceProblem::Missing);
    world.quotas = vec![named("other-quota", |metadata| ResourceQuota {
        metadata,
        ..ResourceQuota::default()
    })];
    assert_eq!(fault_of(&world).object, FenceObject::ResourceQuota);
}

#[test]
fn a_missing_default_deny_is_named() {
    let mut world = fenced_world();
    world.policies.clear();
    let fault = fault_of(&world);
    assert_eq!(fault.object, FenceObject::NetworkPolicy);
    assert_eq!(fault.name, "br-sandbox-default-deny");
    assert_eq!(fault.problem, FenceProblem::Missing);
    assert!(fault.to_string().contains("br-sandbox-default-deny is missing"));
}

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
    ingress.policies[0].spec.as_mut().unwrap().ingress = Some(vec![NetworkPolicyIngressRule::default()]);
    let mut egress = fenced_world();
    egress.policies[0].spec.as_mut().unwrap().egress = Some(vec![NetworkPolicyEgressRule::default()]);
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
    assert!(matches!(fault_of(&world).problem, FenceProblem::NotDefaultDeny(_)));
}

#[test]
fn the_two_other_policies_of_the_closed_list_are_fenced() {
    let mut world = fenced_world();
    world.policies.push(policy(NETWORK_POLICY_EGRESS_ALLOW));
    world.policies.push(policy(NETWORK_POLICY_EGRESS_FULL));
    assert!(verdict(&world).is_fenced());
}

#[test]
fn a_network_policy_outside_the_closed_list_is_named_and_the_namespace_is_not_fenced() {
    let mut world = fenced_world();
    world.policies.push(policy("allow-all-inside"));
    let fault = fault_of(&world);
    assert_eq!(fault.object, FenceObject::NetworkPolicy);
    assert_eq!(fault.name, "allow-all-inside");
    assert_eq!(fault.problem, FenceProblem::NotInClosedList);
    assert!(fault.to_string().contains("allow-all-inside"));
}

#[test]
fn a_missing_binding_is_named_and_a_binding_of_another_name_is_no_binding() {
    let mut world = fenced_world();
    world.binding = None;
    let fault = fault_of(&world);
    assert_eq!(fault.object, FenceObject::AdmissionBinding);
    assert_eq!(fault.name, "br-sandbox-pod");
    assert_eq!(fault.problem, FenceProblem::Missing);
    let mut other = binding(None);
    other.metadata.name = Some("something-else".to_owned());
    world.binding = Some(other);
    assert_eq!(fault_of(&world).problem, FenceProblem::Missing);
}

#[test]
fn a_binding_that_does_not_select_the_namespace_is_not_a_fence() {
    let mut world = fenced_world();
    world.binding = Some(binding(Some(LabelSelector {
        match_labels: Some(BTreeMap::from([("team".to_owned(), "other".to_owned())])),
        match_expressions: None,
    })));
    let fault = fault_of(&world);
    assert_eq!(fault.problem, FenceProblem::NotSelected);
    assert!(fault.to_string().contains("does not select the namespace"));
}

#[test]
fn a_binding_with_no_namespace_selector_selects_every_namespace() {
    let mut world = fenced_world();
    world.binding = Some(binding(None));
    assert!(verdict(&world).is_fenced());
    let mut no_resources = fenced_world();
    no_resources.binding.as_mut().unwrap().spec = Some(ValidatingAdmissionPolicyBindingSpec::default());
    assert!(verdict(&no_resources).is_fenced());
}

#[test]
fn the_first_fault_in_the_order_of_the_table_is_the_one_that_is_named() {
    let mut world = fenced_world();
    world.quotas.clear();
    world.binding = None;
    assert_eq!(fault_of(&world).object, FenceObject::ResourceQuota);
    world.labels.clear();
    assert_eq!(fault_of(&world).object, FenceObject::Namespace);
}
