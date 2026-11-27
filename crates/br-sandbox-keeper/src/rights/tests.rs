use super::{
    NETWORK_POLICY_DEFAULT_DENY, NETWORK_POLICY_EGRESS_ALLOW, NETWORK_POLICY_EGRESS_FULL,
    NETWORK_POLICY_NAMES, POD_TEMPLATE_CONFIGMAP, RIGHTS, Right, RightScope,
};

type Row = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

const EACH: &str = "each managed namespace";
const LEASE: &str = "lease namespace";
const CLUSTER: &str = "cluster";

const INTENT: [Row; 12] = [
    (EACH, "", "pods", &[], &["get", "list", "watch", "delete"]),
    (
        EACH,
        "apps",
        "replicasets",
        &[],
        &["get", "list", "watch", "patch", "create", "delete"],
    ),
    (EACH, "apps", "replicasets/status", &[], &["patch"]),
    (EACH, "", "events", &[], &["create"]),
    (
        EACH,
        "sandbox.botresources.ai",
        "sandboxpools",
        &[],
        &["get", "list", "watch"],
    ),
    (
        EACH,
        "sandbox.botresources.ai",
        "sandboxpools/status",
        &[],
        &["patch"],
    ),
    (
        EACH,
        "",
        "configmaps",
        &["br-sandbox-pod-template"],
        &["get"],
    ),
    (EACH, "", "resourcequotas", &[], &["get", "list", "watch"]),
    (
        EACH,
        "networking.k8s.io",
        "networkpolicies",
        &[],
        &["get", "list", "watch"],
    ),
    (
        LEASE,
        "coordination.k8s.io",
        "leases",
        &[],
        &["get", "create", "update"],
    ),
    (CLUSTER, "", "namespaces", &[], &["get", "list", "watch"]),
    (
        CLUSTER,
        "admissionregistration.k8s.io",
        "validatingadmissionpolicybindings",
        &[],
        &["get", "list", "watch"],
    ),
];

fn scope_name(scope: RightScope) -> &'static str {
    match scope {
        RightScope::EachManagedNamespace => EACH,
        RightScope::LeaseNamespace => LEASE,
        RightScope::Cluster => CLUSTER,
    }
}

fn row_of(right: &Right) -> Row {
    (
        scope_name(right.scope),
        right.api_group,
        right.resource,
        right.resource_names,
        right.verbs,
    )
}

const WRITE_VERBS: [&str; 5] = ["create", "update", "patch", "delete", "deletecollection"];

#[test]
fn the_exported_table_is_the_table_of_the_intent_row_by_row() {
    assert_eq!(RIGHTS.len(), INTENT.len());
    for (index, (right, expected)) in RIGHTS.iter().zip(INTENT).enumerate() {
        assert_eq!(row_of(right), expected, "row {index}");
    }
}

#[test]
fn the_status_rights_are_patch_only_on_the_status_resources() {
    let statuses: Vec<&Right> = RIGHTS
        .iter()
        .filter(|right| right.resource.ends_with("/status"))
        .collect();
    assert_eq!(statuses.len(), 2);
    assert!(statuses.iter().all(|right| right.verbs == ["patch"]));
    let pools: Vec<&Right> = RIGHTS
        .iter()
        .filter(|right| right.resource == "sandboxpools")
        .collect();
    assert_eq!(pools.len(), 1);
    assert_eq!(pools[0].verbs, ["get", "list", "watch"]);
}

#[test]
fn no_row_has_a_wildcard_or_a_verb_that_widens_a_right() {
    for right in RIGHTS {
        for text in [right.api_group, right.resource]
            .into_iter()
            .chain(right.resource_names.iter().copied())
            .chain(right.verbs.iter().copied())
        {
            assert!(!text.contains('*'), "{right:?}");
        }
        for verb in ["escalate", "bind", "impersonate", "approve", "sign"] {
            assert!(!right.verbs.contains(&verb), "{right:?}");
        }
        assert!(!right.verbs.is_empty(), "{right:?}");
        assert!(!right.resource.is_empty(), "{right:?}");
    }
}

#[test]
fn no_row_writes_a_resource_that_the_keeper_only_reads() {
    let never_written = [
        "networkpolicies",
        "namespaces",
        "roles",
        "rolebindings",
        "clusterroles",
        "clusterrolebindings",
        "resourcequotas",
        "validatingadmissionpolicies",
        "validatingadmissionpolicybindings",
        "customresourcedefinitions",
        "secrets",
        "sandboxpools",
        "configmaps",
    ];
    for right in RIGHTS {
        let base = right.resource.split('/').next().unwrap();
        if never_written.contains(&base) && right.resource == base {
            assert!(
                right.verbs.iter().all(|verb| !WRITE_VERBS.contains(verb)),
                "{right:?}"
            );
        }
    }
}

#[test]
fn the_keeper_has_no_right_on_roles_bindings_secrets_exec_or_finalizers() {
    for right in RIGHTS {
        let base = right.resource.split('/').next().unwrap();
        assert!(
            !matches!(
                base,
                "roles"
                    | "rolebindings"
                    | "clusterroles"
                    | "clusterrolebindings"
                    | "secrets"
                    | "validatingadmissionpolicies"
                    | "customresourcedefinitions"
            ),
            "{right:?}"
        );
        assert!(!right.resource.ends_with("/exec"), "{right:?}");
        assert!(!right.resource.ends_with("/finalizers"), "{right:?}");
    }
}

#[test]
fn pods_are_never_created_or_patched_and_the_only_named_resource_is_the_template() {
    let pods: Vec<&Right> = RIGHTS
        .iter()
        .filter(|right| right.resource == "pods")
        .collect();
    assert_eq!(pods.len(), 1);
    assert!(!pods[0].verbs.contains(&"create"));
    assert!(!pods[0].verbs.contains(&"patch"));
    assert!(!pods[0].verbs.contains(&"update"));
    let named: Vec<&Right> = RIGHTS
        .iter()
        .filter(|right| !right.resource_names.is_empty())
        .collect();
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].resource, "configmaps");
    assert_eq!(named[0].resource_names, [POD_TEMPLATE_CONFIGMAP]);
    assert_eq!(POD_TEMPLATE_CONFIGMAP, "br-sandbox-pod-template");
}

#[test]
fn the_closed_list_of_network_policy_names_is_the_one_of_the_intent() {
    assert_eq!(NETWORK_POLICY_DEFAULT_DENY, "br-sandbox-default-deny");
    assert_eq!(NETWORK_POLICY_EGRESS_ALLOW, "br-sandbox-egress-allow");
    assert_eq!(NETWORK_POLICY_EGRESS_FULL, "br-sandbox-egress-full");
    assert_eq!(
        NETWORK_POLICY_NAMES,
        [
            "br-sandbox-default-deny",
            "br-sandbox-egress-allow",
            "br-sandbox-egress-full"
        ]
    );
}

#[test]
fn the_keeper_has_no_right_on_a_sub_resource_that_opens_a_connection_to_a_pod_or_a_node() {
    for right in RIGHTS {
        for connecting in [
            "pods/exec",
            "pods/attach",
            "pods/portforward",
            "pods/proxy",
            "nodes/proxy",
            "services/proxy",
        ] {
            assert_ne!(right.resource, connecting, "{right:?}");
        }
        let sub = right.resource.split_once('/').map(|(_, sub)| sub);
        assert!(
            !matches!(sub, Some("exec" | "attach" | "portforward" | "proxy")),
            "{right:?}"
        );
    }
}
