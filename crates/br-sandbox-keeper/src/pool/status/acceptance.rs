use super::{
    REASON_DUPLICATE_POOL, REASON_INVALID_SPEC, REASON_INVALID_TEMPLATE,
    REASON_NAMESPACE_NOT_FENCED, REASON_NOT_OBSERVED, REASON_TEMPLATE_ABSENT, REASON_VALID,
};
use crate::pool::{FenceVerdict, InvalidSpec, PoolSettings, PoolTemplate, TemplateFault};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Acceptance {
    Valid,
    InvalidSpec(String),
    NamespaceNotFenced(String),
    DuplicatePool,
    TemplateAbsent,
    InvalidTemplate(String),
    NotObserved,
}

impl Acceptance {
    pub fn is_valid(&self) -> bool {
        matches!(self, Acceptance::Valid)
    }

    pub(super) fn fields(&self) -> (&'static str, &'static str, String) {
        match self {
            Acceptance::Valid => (
                "True",
                REASON_VALID,
                "the resource passed its rules, the namespace has the fence and the template is good"
                    .to_owned(),
            ),
            Acceptance::InvalidSpec(message) => {
                ("False", REASON_INVALID_SPEC, message.clone())
            }
            Acceptance::NamespaceNotFenced(message) => {
                ("False", REASON_NAMESPACE_NOT_FENCED, message.clone())
            }
            Acceptance::DuplicatePool => (
                "False",
                REASON_DUPLICATE_POOL,
                "an older SandboxPool of this namespace holds the pool".to_owned(),
            ),
            Acceptance::TemplateAbsent => (
                "False",
                REASON_TEMPLATE_ABSENT,
                "the ConfigMap of the pod template is absent".to_owned(),
            ),
            Acceptance::InvalidTemplate(message) => {
                ("False", REASON_INVALID_TEMPLATE, message.clone())
            }
            Acceptance::NotObserved => (
                "Unknown",
                REASON_NOT_OBSERVED,
                "a view is not synced or a read failed".to_owned(),
            ),
        }
    }
}

pub fn judge_acceptance(
    fence: Option<&FenceVerdict>,
    duplicate: Option<bool>,
    spec: &Result<PoolSettings, InvalidSpec>,
    template: Option<&Result<PoolTemplate, TemplateFault>>,
) -> Acceptance {
    let (Some(fence), Some(duplicate)) = (fence, duplicate) else {
        return Acceptance::NotObserved;
    };
    if let FenceVerdict::NotFenced(fault) = fence {
        return Acceptance::NamespaceNotFenced(fault.to_string());
    }
    if duplicate {
        return Acceptance::DuplicatePool;
    }
    if let Err(error) = spec {
        return Acceptance::InvalidSpec(error.to_string());
    }
    match template {
        None => Acceptance::NotObserved,
        Some(Err(TemplateFault::Absent)) => Acceptance::TemplateAbsent,
        Some(Err(TemplateFault::Invalid(reason))) => Acceptance::InvalidTemplate(reason.clone()),
        Some(Ok(_)) => Acceptance::Valid,
    }
}
