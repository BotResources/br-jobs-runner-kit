use super::{Right, RightScope};

pub const POD_TEMPLATE_CONFIGMAP: &str = "br-sandbox-pod-template";

const fn right(
    scope: RightScope,
    api_group: &'static str,
    resource: &'static str,
    verbs: &'static [&'static str],
) -> Right {
    Right {
        scope,
        api_group,
        resource,
        resource_names: &[],
        verbs,
    }
}

const READ: &[&str] = &["get", "list", "watch"];

pub const RIGHTS: &[Right] = &[
    right(
        RightScope::EachManagedNamespace,
        "",
        "pods",
        &["get", "list", "watch", "delete"],
    ),
    right(
        RightScope::EachManagedNamespace,
        "apps",
        "replicasets",
        &["get", "list", "watch", "patch", "create", "delete"],
    ),
    right(
        RightScope::EachManagedNamespace,
        "apps",
        "replicasets/status",
        &["patch"],
    ),
    right(RightScope::EachManagedNamespace, "", "events", &["create"]),
    right(
        RightScope::EachManagedNamespace,
        crate::resource::GROUP,
        "sandboxpools",
        READ,
    ),
    right(
        RightScope::EachManagedNamespace,
        crate::resource::GROUP,
        "sandboxpools/status",
        &["patch"],
    ),
    Right {
        resource_names: &[POD_TEMPLATE_CONFIGMAP],
        ..right(RightScope::EachManagedNamespace, "", "configmaps", &["get"])
    },
    right(RightScope::EachManagedNamespace, "", "resourcequotas", READ),
    right(
        RightScope::EachManagedNamespace,
        "networking.k8s.io",
        "networkpolicies",
        READ,
    ),
    right(
        RightScope::LeaseNamespace,
        "coordination.k8s.io",
        "leases",
        &["get", "create", "update"],
    ),
    right(RightScope::Cluster, "", "namespaces", READ),
    right(
        RightScope::Cluster,
        "admissionregistration.k8s.io",
        "validatingadmissionpolicybindings",
        READ,
    ),
];
