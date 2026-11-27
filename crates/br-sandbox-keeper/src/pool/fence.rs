use std::collections::BTreeMap;

use k8s_openapi::api::admissionregistration::v1::ValidatingAdmissionPolicyBinding;
use k8s_openapi::api::core::v1::ResourceQuota;
use k8s_openapi::api::networking::v1::NetworkPolicy;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;

use super::names::{ADMISSION_BINDING_NAME, QUOTA_NAME};
use super::selector::Selector;
use crate::rights::{NETWORK_POLICY_DEFAULT_DENY, NETWORK_POLICY_NAMES};

#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct FenceFacts<'a> {
    pub namespace: &'a str,
    pub labels: &'a BTreeMap<String, String>,
    pub managed_selector: &'a str,
    pub quotas: &'a [ResourceQuota],
    pub network_policies: &'a [NetworkPolicy],
    pub binding: Option<&'a ValidatingAdmissionPolicyBinding>,
}

impl<'a> FenceFacts<'a> {
    pub fn new(
        namespace: &'a str,
        labels: &'a BTreeMap<String, String>,
        managed_selector: &'a str,
        quotas: &'a [ResourceQuota],
        network_policies: &'a [NetworkPolicy],
        binding: Option<&'a ValidatingAdmissionPolicyBinding>,
    ) -> Self {
        Self {
            namespace,
            labels,
            managed_selector,
            quotas,
            network_policies,
            binding,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FenceObject {
    Namespace,
    ResourceQuota,
    NetworkPolicy,
    AdmissionBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FenceProblem {
    Missing,
    SelectorUnreadable(String),
    NotSelected,
    NotDefaultDeny(&'static str),
    NotInClosedList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FenceFault {
    pub object: FenceObject,
    pub name: String,
    pub problem: FenceProblem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FenceVerdict {
    Fenced,
    NotFenced(FenceFault),
}

impl FenceVerdict {
    pub fn is_fenced(&self) -> bool {
        matches!(self, FenceVerdict::Fenced)
    }
}

impl std::fmt::Display for FenceFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.object {
            FenceObject::Namespace => "the namespace",
            FenceObject::ResourceQuota => "the ResourceQuota",
            FenceObject::NetworkPolicy => "the NetworkPolicy",
            FenceObject::AdmissionBinding => "the ValidatingAdmissionPolicyBinding",
        };
        let name = &self.name;
        match &self.problem {
            FenceProblem::Missing => write!(f, "{kind} {name} is missing"),
            FenceProblem::SelectorUnreadable(reason) => {
                write!(f, "{kind} {name} cannot be judged: {reason}")
            }
            FenceProblem::NotSelected => match self.object {
                FenceObject::Namespace => write!(
                    f,
                    "{kind} {name} does not match the managed namespace selector"
                ),
                _ => write!(f, "{kind} {name} does not select the namespace"),
            },
            FenceProblem::NotDefaultDeny(reason) => {
                write!(f, "{kind} {name} is not a default-deny policy: {reason}")
            }
            FenceProblem::NotInClosedList => write!(
                f,
                "{kind} {name} is not one of the network policies of the sandbox"
            ),
        }
    }
}

fn fault(object: FenceObject, name: &str, problem: FenceProblem) -> FenceVerdict {
    FenceVerdict::NotFenced(FenceFault {
        object,
        name: name.to_owned(),
        problem,
    })
}

fn is_empty_selector(selector: &LabelSelector) -> bool {
    selector.match_labels.as_ref().is_none_or(BTreeMap::is_empty)
        && selector
            .match_expressions
            .as_ref()
            .is_none_or(Vec::is_empty)
}

fn default_deny_shape(policy: &NetworkPolicy) -> Result<(), &'static str> {
    let Some(spec) = policy.spec.as_ref() else {
        return Err("it has no spec");
    };
    if !spec.pod_selector.as_ref().is_none_or(is_empty_selector) {
        return Err("it does not select all pods");
    }
    let types = spec.policy_types.as_deref().unwrap_or_default();
    for wanted in ["Ingress", "Egress"] {
        if !types.iter().any(|kind| kind == wanted) {
            return Err("its types do not hold Ingress and Egress");
        }
    }
    if spec.ingress.as_ref().is_some_and(|rules| !rules.is_empty())
        || spec.egress.as_ref().is_some_and(|rules| !rules.is_empty())
    {
        return Err("it has an allow rule");
    }
    Ok(())
}

fn name_of(policy: &NetworkPolicy) -> &str {
    policy.metadata.name.as_deref().unwrap_or_default()
}

fn namespace_check(facts: &FenceFacts<'_>) -> Option<FenceVerdict> {
    match Selector::parse(facts.managed_selector) {
        Err(error) => Some(fault(
            FenceObject::Namespace,
            facts.namespace,
            FenceProblem::SelectorUnreadable(error.to_string()),
        )),
        Ok(selector) if !selector.matches(facts.labels) => Some(fault(
            FenceObject::Namespace,
            facts.namespace,
            FenceProblem::NotSelected,
        )),
        Ok(_) => None,
    }
}

fn quota_check(facts: &FenceFacts<'_>) -> Option<FenceVerdict> {
    let found = facts
        .quotas
        .iter()
        .any(|quota| quota.metadata.name.as_deref() == Some(QUOTA_NAME));
    (!found).then(|| fault(FenceObject::ResourceQuota, QUOTA_NAME, FenceProblem::Missing))
}

fn policies_check(facts: &FenceFacts<'_>) -> Option<FenceVerdict> {
    let Some(deny) = facts
        .network_policies
        .iter()
        .find(|policy| name_of(policy) == NETWORK_POLICY_DEFAULT_DENY)
    else {
        return Some(fault(
            FenceObject::NetworkPolicy,
            NETWORK_POLICY_DEFAULT_DENY,
            FenceProblem::Missing,
        ));
    };
    if let Err(reason) = default_deny_shape(deny) {
        return Some(fault(
            FenceObject::NetworkPolicy,
            NETWORK_POLICY_DEFAULT_DENY,
            FenceProblem::NotDefaultDeny(reason),
        ));
    }
    facts
        .network_policies
        .iter()
        .map(name_of)
        .find(|name| !NETWORK_POLICY_NAMES.contains(name))
        .map(|name| fault(FenceObject::NetworkPolicy, name, FenceProblem::NotInClosedList))
}

fn binding_check(facts: &FenceFacts<'_>) -> Option<FenceVerdict> {
    let named = facts
        .binding
        .filter(|binding| binding.metadata.name.as_deref() == Some(ADMISSION_BINDING_NAME));
    let Some(binding) = named else {
        return Some(fault(
            FenceObject::AdmissionBinding,
            ADMISSION_BINDING_NAME,
            FenceProblem::Missing,
        ));
    };
    let selector = binding
        .spec
        .as_ref()
        .and_then(|spec| spec.match_resources.as_ref())
        .and_then(|resources| resources.namespace_selector.as_ref());
    let selector = selector?;
    match Selector::from_label_selector(selector) {
        Err(error) => Some(fault(
            FenceObject::AdmissionBinding,
            ADMISSION_BINDING_NAME,
            FenceProblem::SelectorUnreadable(error.to_string()),
        )),
        Ok(parsed) if !parsed.matches(facts.labels) => Some(fault(
            FenceObject::AdmissionBinding,
            ADMISSION_BINDING_NAME,
            FenceProblem::NotSelected,
        )),
        Ok(_) => None,
    }
}

pub fn fence_verdict(facts: &FenceFacts<'_>) -> FenceVerdict {
    namespace_check(facts)
        .or_else(|| quota_check(facts))
        .or_else(|| policies_check(facts))
        .or_else(|| binding_check(facts))
        .unwrap_or(FenceVerdict::Fenced)
}

#[cfg(test)]
mod tests;
