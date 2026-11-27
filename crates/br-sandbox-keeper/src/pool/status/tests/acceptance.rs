use std::collections::BTreeMap;

use super::super::*;
use crate::pool::{
    FenceFault, FenceObject, FenceProblem, FenceVerdict, InvalidSpec, PoolSettings, PoolTemplate,
    TemplateFault, validate_spec,
};

fn settings() -> PoolSettings {
    crate::testing::settings()
}

fn good_spec() -> Result<PoolSettings, InvalidSpec> {
    Ok(settings())
}

fn bad_spec() -> Result<PoolSettings, InvalidSpec> {
    let mut spec = crate::resource::SandboxPoolSpec::new(
        "",
        crate::resource::PoolSize::new(
            crate::resource::SizeBounds::new("1", "1"),
            crate::resource::SizeBounds::new("1", "1"),
            crate::resource::SizeBounds::new("1", "1"),
        ),
        0,
        1,
    );
    spec.image = String::new();
    validate_spec(&spec)
}

fn template_ok() -> Result<PoolTemplate, TemplateFault> {
    let keys = br_runner_sandbox::PodKeys::default();
    let map = k8s_openapi::api::core::v1::ConfigMap {
        data: Some(BTreeMap::from([(
            crate::pool::POD_TEMPLATE_KEY.to_owned(),
            serde_json::json!({
                "metadata": {"labels": {"br.sandbox/pool": "warm", "br.sandbox/pool-id": "sbx"}},
                "spec": {"containers": [{"name": "agent"}]}
            })
            .to_string(),
        )])),
        ..Default::default()
    };
    PoolTemplate::read(Some(&map), "sbx", &keys, "agent")
}

fn not_fenced() -> FenceVerdict {
    FenceVerdict::NotFenced(FenceFault {
        object: FenceObject::ResourceQuota,
        name: "br-sandbox-quota".to_owned(),
        problem: FenceProblem::Missing,
    })
}

#[test]
fn a_resource_that_passes_every_check_is_accepted() {
    let template = template_ok();
    let accepted = judge_acceptance(
        Some(&FenceVerdict::Fenced),
        Some(false),
        &good_spec(),
        Some(&template),
    );
    assert_eq!(accepted, Acceptance::Valid);
    assert!(accepted.is_valid());
}

#[test]
fn a_view_that_is_not_synced_or_a_read_that_failed_gives_not_observed() {
    let template = template_ok();
    assert_eq!(
        judge_acceptance(None, Some(false), &good_spec(), Some(&template)),
        Acceptance::NotObserved
    );
    assert_eq!(
        judge_acceptance(
            Some(&FenceVerdict::Fenced),
            None,
            &good_spec(),
            Some(&template)
        ),
        Acceptance::NotObserved
    );
    assert_eq!(
        judge_acceptance(Some(&FenceVerdict::Fenced), Some(false), &good_spec(), None),
        Acceptance::NotObserved
    );
}

#[test]
fn a_namespace_that_is_not_fenced_names_the_object_in_the_message() {
    let verdict = not_fenced();
    let accepted = judge_acceptance(Some(&verdict), Some(false), &good_spec(), None);
    match accepted {
        Acceptance::NamespaceNotFenced(message) => {
            assert!(message.contains("br-sandbox-quota"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_absent_template_is_template_absent_and_never_a_fence_fault() {
    let absent = Err(TemplateFault::Absent);
    let accepted = judge_acceptance(
        Some(&FenceVerdict::Fenced),
        Some(false),
        &good_spec(),
        Some(&absent),
    );
    assert_eq!(accepted, Acceptance::TemplateAbsent);
    let (status, reason, _) = accepted.fields();
    assert_eq!((status, reason), ("False", "TemplateAbsent"));
}

#[test]
fn an_invalid_template_carries_its_reason() {
    let invalid = Err(TemplateFault::Invalid(
        "no container is named agent".to_owned(),
    ));
    let accepted = judge_acceptance(
        Some(&FenceVerdict::Fenced),
        Some(false),
        &good_spec(),
        Some(&invalid),
    );
    assert_eq!(
        accepted,
        Acceptance::InvalidTemplate("no container is named agent".to_owned())
    );
}

#[test]
fn an_invalid_spec_names_the_field_and_the_reason() {
    let accepted = judge_acceptance(Some(&FenceVerdict::Fenced), Some(false), &bad_spec(), None);
    assert_eq!(
        accepted,
        Acceptance::InvalidSpec("image: it is blank".to_owned())
    );
}

#[test]
fn a_second_resource_of_the_namespace_is_a_duplicate_pool() {
    let accepted = judge_acceptance(Some(&FenceVerdict::Fenced), Some(true), &good_spec(), None);
    assert_eq!(accepted, Acceptance::DuplicatePool);
}

#[test]
fn the_order_of_the_faults_is_fence_then_duplicate_then_spec_then_template() {
    let template = Err(TemplateFault::Absent);
    let verdict = not_fenced();
    assert!(matches!(
        judge_acceptance(Some(&verdict), Some(true), &bad_spec(), Some(&template)),
        Acceptance::NamespaceNotFenced(_)
    ));
    assert_eq!(
        judge_acceptance(
            Some(&FenceVerdict::Fenced),
            Some(true),
            &bad_spec(),
            Some(&template)
        ),
        Acceptance::DuplicatePool
    );
    assert!(matches!(
        judge_acceptance(
            Some(&FenceVerdict::Fenced),
            Some(false),
            &bad_spec(),
            Some(&template)
        ),
        Acceptance::InvalidSpec(_)
    ));
}

#[test]
fn each_reason_of_the_table_is_the_text_of_the_intent() {
    let cases = [
        (Acceptance::Valid, "True", "Valid"),
        (
            Acceptance::InvalidSpec("x".to_owned()),
            "False",
            "InvalidSpec",
        ),
        (
            Acceptance::NamespaceNotFenced("x".to_owned()),
            "False",
            "NamespaceNotFenced",
        ),
        (Acceptance::DuplicatePool, "False", "DuplicatePool"),
        (Acceptance::TemplateAbsent, "False", "TemplateAbsent"),
        (
            Acceptance::InvalidTemplate("x".to_owned()),
            "False",
            "InvalidTemplate",
        ),
        (Acceptance::NotObserved, "Unknown", "NotObserved"),
    ];
    for (acceptance, status, reason) in cases {
        let (got_status, got_reason, _) = acceptance.fields();
        assert_eq!((got_status, got_reason), (status, reason));
    }
}
