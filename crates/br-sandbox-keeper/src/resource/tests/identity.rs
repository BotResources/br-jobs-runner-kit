use kube::Resource;

use super::super::{GROUP, KIND, LIST_KIND, PLURAL, SINGULAR, SandboxPool, VERSION, definition};

#[test]
fn the_resource_has_the_names_of_the_intent() {
    assert_eq!(GROUP, "sandbox.botresources.ai");
    assert_eq!(VERSION, "v1alpha1");
    assert_eq!(KIND, "SandboxPool");
    assert_eq!(PLURAL, "sandboxpools");
    assert_eq!(SandboxPool::group(&()), GROUP);
    assert_eq!(SandboxPool::version(&()), VERSION);
    assert_eq!(SandboxPool::kind(&()), KIND);
    assert_eq!(SandboxPool::plural(&()), PLURAL);
    assert_eq!(
        SandboxPool::api_version(&()),
        "sandbox.botresources.ai/v1alpha1"
    );
}

#[test]
fn the_resource_is_namespaced_in_its_url() {
    assert_eq!(
        SandboxPool::url_path(&(), Some("sbx")),
        "/apis/sandbox.botresources.ai/v1alpha1/namespaces/sbx/sandboxpools"
    );
}

#[test]
fn the_definition_has_one_namespaced_version_that_is_served_and_stored() {
    let crd = definition();
    assert_eq!(
        crd.metadata.name.as_deref(),
        Some("sandboxpools.sandbox.botresources.ai")
    );
    assert_eq!(crd.spec.group, GROUP);
    assert_eq!(crd.spec.scope, "Namespaced");
    assert_eq!(crd.spec.names.kind, KIND);
    assert_eq!(crd.spec.names.plural, PLURAL);
    assert_eq!(crd.spec.names.singular.as_deref(), Some(SINGULAR));
    assert_eq!(crd.spec.names.list_kind.as_deref(), Some(LIST_KIND));
    assert_eq!(crd.spec.versions.len(), 1);
    let version = &crd.spec.versions[0];
    assert_eq!(version.name, VERSION);
    assert!(version.served);
    assert!(version.storage);
}

#[test]
fn the_definition_has_a_status_subresource_and_no_scale_subresource() {
    let version = &definition().spec.versions[0];
    let subresources = version.subresources.as_ref().unwrap();
    assert!(subresources.status.is_some());
    assert!(subresources.scale.is_none());
}

#[test]
fn the_definition_has_the_four_printer_columns_of_the_intent() {
    let version = &definition().spec.versions[0];
    let columns: Vec<(&str, &str, &str)> = version
        .additional_printer_columns
        .as_ref()
        .unwrap()
        .iter()
        .map(|column| {
            (
                column.name.as_str(),
                column.type_.as_str(),
                column.json_path.as_str(),
            )
        })
        .collect();
    assert_eq!(
        columns,
        vec![
            ("Desired", "integer", ".status.desiredReplicas"),
            ("Ready", "integer", ".status.readyReplicas"),
            (
                "Accepted",
                "string",
                ".status.conditions[?(@.type==\"Accepted\")].status"
            ),
            ("Age", "date", ".metadata.creationTimestamp"),
        ]
    );
}
