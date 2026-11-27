use br_sandbox_keeper::pool::{ADMISSION_BINDING_NAME, POD_TEMPLATE_KEY, QUOTA_NAME};
use br_sandbox_keeper::rights::POD_TEMPLATE_CONFIGMAP;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

mod common;

use common::{chart, rendered};

const CONTROLLER_ROLE: &str = "br-sandbox-controller";

fn overlays() -> Vec<PathBuf> {
    ["operator-values.yaml", "fence-values.yaml"]
        .iter()
        .map(|name| chart().join("ci").join(name))
        .collect()
}

fn of_kind<'a>(objects: &'a [Value], kind: &str) -> Vec<&'a Value> {
    objects
        .iter()
        .filter(|object| object["kind"] == kind)
        .collect()
}

fn names(objects: &[&Value]) -> Vec<String> {
    let mut names: Vec<String> = objects
        .iter()
        .map(|object| {
            object["metadata"]["name"]
                .as_str()
                .expect("a name")
                .to_owned()
        })
        .collect();
    names.sort();
    names
}

fn managed_namespaces(objects: &[Value]) -> BTreeSet<String> {
    objects
        .iter()
        .filter(|object| {
            object["kind"] == "RoleBinding" && object["roleRef"]["name"] == CONTROLLER_ROLE
        })
        .map(|object| {
            object["metadata"]["namespace"]
                .as_str()
                .expect("a namespace")
                .to_owned()
        })
        .collect()
}

fn in_namespace<'a>(objects: &'a [Value], kind: &str, namespace: &str) -> Vec<&'a Value> {
    of_kind(objects, kind)
        .into_iter()
        .filter(|object| object["metadata"]["namespace"] == namespace)
        .collect()
}

#[test]
fn the_admission_policy_and_its_create_binding_carry_the_name_that_the_keeper_reads() {
    let objects = rendered(&chart().join("ci/operator-values.yaml"), &[]);
    let policies = of_kind(&objects, "ValidatingAdmissionPolicy");
    assert!(names(&policies).contains(&ADMISSION_BINDING_NAME.to_owned()));
    let bindings: Vec<&Value> = of_kind(&objects, "ValidatingAdmissionPolicyBinding")
        .into_iter()
        .filter(|binding| binding["metadata"]["name"] == ADMISSION_BINDING_NAME)
        .collect();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0]["spec"]["policyName"], ADMISSION_BINDING_NAME);
}

#[test]
fn every_managed_namespace_holds_one_quota_with_the_name_that_the_keeper_reads() {
    for overlay in overlays() {
        let objects = rendered(&overlay, &[]);
        let managed = managed_namespaces(&objects);
        assert!(managed.len() >= 3, "{managed:?}");
        for namespace in &managed {
            let quotas = in_namespace(&objects, "ResourceQuota", namespace);
            assert_eq!(
                names(&quotas),
                [QUOTA_NAME],
                "{namespace} {}",
                overlay.display()
            );
        }
    }
}

#[test]
fn every_managed_namespace_holds_one_template_config_map_with_the_name_and_key_that_the_keeper_reads()
 {
    for overlay in overlays() {
        let objects = rendered(&overlay, &[]);
        let managed = managed_namespaces(&objects);
        assert!(managed.len() >= 3, "{managed:?}");
        for namespace in &managed {
            let maps: Vec<&Value> = in_namespace(&objects, "ConfigMap", namespace)
                .into_iter()
                .filter(|object| object["metadata"]["name"] == POD_TEMPLATE_CONFIGMAP)
                .collect();
            assert_eq!(maps.len(), 1, "{namespace} {}", overlay.display());
            let keys: Vec<&String> = maps[0]["data"].as_object().expect("data").keys().collect();
            assert_eq!(keys, [POD_TEMPLATE_KEY], "{namespace}");
        }
    }
}
