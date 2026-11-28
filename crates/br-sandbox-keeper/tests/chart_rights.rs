use br_sandbox_keeper::rights::{NETWORK_POLICY_NAMES, RIGHTS, Right, RightScope};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

mod common;

use common::{chart, rendered};

type Row = (String, String, Vec<String>, Vec<String>);

const KEEPER_SERVICE_ACCOUNT: &str = "br-sandbox-keeper";
const CONTROLLER_ROLE: &str = "br-sandbox-controller";
const LEASE_ROLE: &str = "br-sandbox-keeper-lease";
const CLUSTER_ROLE_SUFFIX: &str = "-keeper-namespaces";

fn overlays() -> Vec<PathBuf> {
    ["operator-values.yaml", "operator-nondefault-values.yaml"]
        .iter()
        .map(|name| chart().join("ci").join(name))
        .collect()
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item.as_str().expect("a string").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn rows_of_the_table(scope: RightScope) -> Vec<Row> {
    let mut rows: Vec<Row> = RIGHTS
        .iter()
        .filter(|right: &&Right| right.scope == scope)
        .map(|right| {
            (
                right.api_group.to_owned(),
                right.resource.to_owned(),
                sorted(
                    right
                        .resource_names
                        .iter()
                        .map(|name| (*name).to_owned())
                        .collect(),
                ),
                sorted(right.verbs.iter().map(|verb| (*verb).to_owned()).collect()),
            )
        })
        .collect();
    rows.sort();
    rows
}

fn rows_of_the_rules(object: &Value) -> Vec<Row> {
    let mut rows = Vec::new();
    for rule in object["rules"].as_array().expect("the role has rules") {
        let groups = strings(&rule["apiGroups"]);
        let resources = strings(&rule["resources"]);
        let names = sorted(strings(&rule["resourceNames"]));
        let verbs = sorted(strings(&rule["verbs"]));
        for group in &groups {
            for resource in &resources {
                rows.push((
                    group.clone(),
                    resource.clone(),
                    names.clone(),
                    verbs.clone(),
                ));
            }
        }
    }
    rows.sort();
    rows
}

fn named<'a>(objects: &'a [Value], kind: &str, name: &str) -> Vec<&'a Value> {
    objects
        .iter()
        .filter(|object| object["kind"] == kind && object["metadata"]["name"] == name)
        .collect()
}

fn one<'a>(objects: &'a [Value], kind: &str, name: &str) -> &'a Value {
    let found = named(objects, kind, name);
    assert_eq!(found.len(), 1, "{kind} {name}");
    found[0]
}

fn bindings_of_the_keeper(objects: &[Value]) -> Vec<&Value> {
    objects
        .iter()
        .filter(|object| {
            matches!(
                object["kind"].as_str(),
                Some("RoleBinding" | "ClusterRoleBinding")
            )
        })
        .filter(|object| {
            object["subjects"].as_array().is_some_and(|subjects| {
                subjects.iter().any(|subject| {
                    subject["kind"] == "ServiceAccount" && subject["name"] == KEEPER_SERVICE_ACCOUNT
                })
            })
        })
        .collect()
}

fn cluster_binding(objects: &[Value]) -> &Value {
    let found: Vec<&Value> = objects
        .iter()
        .filter(|object| {
            object["kind"] == "ClusterRoleBinding"
                && object["metadata"]["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with(CLUSTER_ROLE_SUFFIX))
        })
        .collect();
    assert_eq!(found.len(), 1);
    found[0]
}

#[test]
fn the_rights_of_the_namespaced_rows_are_the_rules_of_the_controller_cluster_role() {
    for overlay in overlays() {
        let objects = rendered(&overlay, &[]);
        let role = one(&objects, "ClusterRole", CONTROLLER_ROLE);
        assert_eq!(
            rows_of_the_rules(role),
            rows_of_the_table(RightScope::EachManagedNamespace),
            "{}",
            overlay.display()
        );
    }
}

#[test]
fn the_controller_cluster_role_is_bound_by_role_bindings_and_by_no_cluster_binding() {
    for overlay in overlays() {
        let objects = rendered(&overlay, &[]);
        let bindings: Vec<&Value> = objects
            .iter()
            .filter(|object| {
                object["kind"] == "RoleBinding" && object["roleRef"]["name"] == CONTROLLER_ROLE
            })
            .collect();
        assert!(!bindings.is_empty(), "{}", overlay.display());
        for binding in bindings {
            assert_eq!(binding["roleRef"]["kind"], "ClusterRole");
            assert_eq!(binding["subjects"][0]["name"], KEEPER_SERVICE_ACCOUNT);
        }
        assert!(
            !objects.iter().any(|object| {
                object["kind"] == "ClusterRoleBinding"
                    && object["roleRef"]["name"] == CONTROLLER_ROLE
            }),
            "{}",
            overlay.display()
        );
    }
}

#[test]
fn the_rights_of_the_lease_row_are_the_rules_of_the_lease_role_of_the_system_namespace() {
    for overlay in overlays() {
        let objects = rendered(&overlay, &[]);
        let role = one(&objects, "Role", LEASE_ROLE);
        assert_eq!(
            rows_of_the_rules(role),
            rows_of_the_table(RightScope::LeaseNamespace),
            "{}",
            overlay.display()
        );
        let binding = one(&objects, "RoleBinding", LEASE_ROLE);
        assert_eq!(
            binding["metadata"]["namespace"],
            role["metadata"]["namespace"]
        );
        assert_eq!(binding["roleRef"]["kind"], "Role");
    }
}

#[test]
fn the_rights_of_the_cluster_rows_are_the_rules_of_the_cluster_role_bound_by_a_cluster_binding() {
    for overlay in overlays() {
        let objects = rendered(&overlay, &[]);
        let binding = cluster_binding(&objects);
        let role = one(
            &objects,
            "ClusterRole",
            binding["roleRef"]["name"].as_str().expect("a role name"),
        );
        assert_eq!(
            rows_of_the_rules(role),
            rows_of_the_table(RightScope::Cluster),
            "{}",
            overlay.display()
        );
    }
}

#[test]
fn the_keeper_service_account_is_bound_to_these_three_roles_and_to_no_other() {
    for overlay in overlays() {
        let objects = rendered(&overlay, &[]);
        let bound: BTreeSet<String> = bindings_of_the_keeper(&objects)
            .iter()
            .map(|binding| {
                binding["roleRef"]["name"]
                    .as_str()
                    .expect("a role name")
                    .to_owned()
            })
            .collect();
        let cluster_role = cluster_binding(&objects)["roleRef"]["name"]
            .as_str()
            .expect("a role name")
            .to_owned();
        let expected: BTreeSet<String> = [CONTROLLER_ROLE, LEASE_ROLE]
            .iter()
            .map(|name| (*name).to_owned())
            .chain(std::iter::once(cluster_role))
            .collect();
        assert_eq!(bound, expected, "{}", overlay.display());
    }
}

#[test]
fn the_network_policies_of_the_managed_namespaces_are_the_closed_names_of_the_keeper() {
    let overlay = chart().join("ci/fence-values.yaml");
    let objects = rendered(&overlay, &[]);
    let managed: BTreeSet<String> = objects
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
        .collect();
    assert!(managed.len() >= 3, "{managed:?}");
    let rendered_names: BTreeSet<String> = objects
        .iter()
        .filter(|object| object["kind"] == "NetworkPolicy")
        .filter(|object| {
            object["metadata"]["namespace"]
                .as_str()
                .is_some_and(|namespace| managed.contains(namespace))
        })
        .map(|object| {
            object["metadata"]["name"]
                .as_str()
                .expect("a name")
                .to_owned()
        })
        .collect();
    let closed: BTreeSet<String> = NETWORK_POLICY_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    assert_eq!(rendered_names, closed);
}
