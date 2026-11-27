#!/usr/bin/env bash

CLUSTER_SCOPED_KINDS='["Namespace", "ClusterRole", "ClusterRoleBinding", "CustomResourceDefinition", "ValidatingAdmissionPolicy", "ValidatingAdmissionPolicyBinding"]'

fence_entries() {
    vals "$1" ".operator.enabled = false | .network_check.enabled = false | .render_namespace = false | .namespaces = $2"
}

cluster_objects() {
    jqv "$1" '[.[] | select(.kind | IN("ClusterRole", "ClusterRoleBinding", "CustomResourceDefinition")) | .kind + "/" + .metadata.name] | sort | join(",")'
}

render_fails() {
    helm template t "$CHART" "$@" >/dev/null 2>&1 && return 1
    return 0
}

check_a_keeper_key_is_a_named_render_fault() {
    section "the keeper values are removed: a keeper key is a named render fault"
    fails_with "BR_SANDBOX_KEEPER_VALUES_REMOVED{key=keeper}" -f "$EXAMPLE" --set keeper.enabled=true
    fails_with "BR_SANDBOX_KEEPER_VALUES_REMOVED{key=keeper}" -f "$EXAMPLE" -f "$(vals keeperkey '.keeper = {"enabled": false}')"
}

check_the_operator_part_renders_without_a_namespace() {
    section "the operator part: with no namespace it renders the controller objects and no sandbox namespace object"
    t "the render holds the keeper Deployment, its document and the system namespace" jqt platform '([.[] | select(.kind == "Deployment" and .metadata.name == "br-sandbox-keeper")] | length == 1) and ([.[] | select(.kind == "ConfigMap" and .metadata.name == "br-sandbox-keeper-config")] | length == 1) and ([.[] | select(.kind == "Namespace") | .metadata.name] == ["sbx-system"])'
    t "the render holds the controller ClusterRole, the runner ClusterRole and the cluster binding of the keeper" jqt platform '[.[] | select(.kind == "ClusterRole" or .kind == "ClusterRoleBinding") | .kind + "/" + .metadata.name] | sort | IN(["ClusterRole/br-sandbox-controller", "ClusterRole/br-sandbox-runner", "ClusterRole/sbx-system-keeper-namespaces", "ClusterRole/sbx-system-netcheck", "ClusterRoleBinding/sbx-system-keeper-namespaces", "ClusterRoleBinding/sbx-system-netcheck"])'
    t "the render holds no object in a sandbox namespace and no binding of a sandbox namespace" jqt platform '[.[] | select((.metadata.namespace // "sbx-system") != "sbx-system" or .kind == "RoleBinding" and .metadata.name != "br-sandbox-keeper-lease")] | length == 0'
    t "the render holds no ServiceAccount named sandbox, no quota, no pod template and no adapter document" jqt platform '[.[] | select((.kind == "ServiceAccount" and .metadata.name == "sandbox") or .kind == "ResourceQuota" or .kind == "LimitRange" or (.kind == "ConfigMap" and (.metadata.name | IN("br-sandbox-pod-template", "br-sandbox-config"))))] | length == 0'
    t "the render holds no admission policy until a namespace is declared" jqt platform '[.[] | select(.kind | startswith("ValidatingAdmission"))] | length == 0'
}

check_nothing_to_render_and_a_part_that_needs_the_operator() {
    section "both parts off and no namespace is a fault; the network check needs the operator part"
    fails_with "BR_SANDBOX_NOTHING_TO_RENDER" -f "$(vals nothing '.operator.enabled = false | .network_check.enabled = false | .namespaces = []')"
    fails_with "BR_SANDBOX_NOTHING_TO_RENDER" -f "$EXAMPLE" --set operator.enabled=false --set network_check.enabled=false --set 'namespaces=null'
    t "the definition alone is something to render" render onlycrd -f "$(vals onlycrd '.operator.enabled = false | .network_check.enabled = false | .namespaces = [] | .crds.install = true')"
    t "namespaces alone are something to render" render onlyfence -f "$(fence_entries onlyfence '[{"name": "sbx-alpha-none", "mode": "none"}]')"
    fails_with "BR_SANDBOX_OPERATOR_PART_REQUIRED{key=network_check}" -f "$EXAMPLE" --set operator.enabled=false
    t "the network check is off by default" test "$(yq '.network_check.enabled' "$CHART/values.yaml")" = false
}

check_the_definition_alone_renders_one_object() {
    section "crds.install alone renders the definition and nothing else, and needs no other value"
    t "the chart without any value renders one object with crds.install" test "$(helm template t "$CHART" --set crds.install=true | yq ea '[select(. != null)] | length')" = 1
    t "the chart without any value and without crds.install renders nothing but the fault" render_fails
    render crdonly -f "$EXAMPLE" -f "$CHART/ci/crds-values.yaml"
    t "the fixture crds-values renders the definition only" jqt crdonly 'length == 1 and .[0].kind == "CustomResourceDefinition"'
}

check_no_render_holds_a_replica_set_a_pool_or_a_crds_folder() {
    section "no render holds a ReplicaSet or a SandboxPool; the chart has no crds folder"
    local name
    for name in ex fence mirror keeper nondefault egress_keeper cred two keeperegress platform rights_five; do
        t "the render ${name} holds no ReplicaSet and no SandboxPool" jqt "$name" '[.[] | select(.kind == "ReplicaSet" or .kind == "SandboxPool")] | length == 0'
    done
    t "the chart has no crds folder" test ! -d "$CHART/crds"
}

check_a_removed_value_is_refused() {
    section "a key that left the chart is refused"
    local assignment
    for assignment in clients.alpha.cpu=1 pool.floor=1 pool.refill_seconds=3 pool.margin=2 pool.max_sandboxes=20 pool.size.cpu.limit=3 image=registry.example.test/agents/base:1.0 operator.bind_roles=true; do
        t "the key ${assignment%%=*} is refused" render_fails -f "$EXAMPLE" --set "$assignment"
    done
}

check_the_fence_part_renders_only_namespaced_objects() {
    section "the fence part: an entry of mode none renders the namespaced objects of the list and nothing else"
    render fence_none -f "$(fence_entries fencenone '[{"name": "sbx-alpha-none", "mode": "none"}]')"
    t "an entry of mode none renders exactly the objects of the fence" test "$(jqv fence_none '[.[] | .kind + "/" + .metadata.name] | sort | join(",")')" = "ConfigMap/br-sandbox-config,ConfigMap/br-sandbox-pod-template,LimitRange/br-sandbox-limits,NetworkPolicy/br-sandbox-default-deny,ResourceQuota/br-sandbox-quota,RoleBinding/br-sandbox-keeper,RoleBinding/br-sandbox-runner,ServiceAccount/sandbox"
    render fence_full -f "$(fence_entries fencefull '[{"name": "sbx-alpha-full", "mode": "full"}]')"
    t "an entry of mode full without a credential set adds the policy br-sandbox-egress-full" test "$(jqv fence_full '[.[] | .kind + "/" + .metadata.name] | sort | join(",")')" = "ConfigMap/br-sandbox-config,ConfigMap/br-sandbox-pod-template,LimitRange/br-sandbox-limits,NetworkPolicy/br-sandbox-default-deny,NetworkPolicy/br-sandbox-egress-full,ResourceQuota/br-sandbox-quota,RoleBinding/br-sandbox-keeper,RoleBinding/br-sandbox-runner,ServiceAccount/sandbox"
    local name
    for name in fence_none fence_full; do
        t "the render ${name} holds no cluster-scoped object and every object is in the namespace of its entry" jqt "$name" "[.[] | select((.kind | IN(${CLUSTER_SCOPED_KINDS}[])) or (.metadata.namespace | startswith(\"sbx-alpha-\") | not))] | length == 0"
    done
    t "the fence render holds no Namespace by default" jqt fence_none '[.[] | select(.kind == "Namespace")] | length == 0'
    render fence_ns --set render_namespace=true -f "$(fence_entries fencens '[{"name": "sbx-alpha-none", "mode": "none"}]')"
    t "with render_namespace the fence render holds the Namespace of its entry with the managed label and the mode" jqt fence_ns '[.[] | select(.kind == "Namespace") | [.metadata.name, .metadata.labels["br.sandbox/managed"], .metadata.labels["br.sandbox/mode"]]] == [["sbx-alpha-none", "true", "none"]]'
}

check_an_entry_takes_a_name_and_a_mode_only() {
    section "an entry takes a name and a mode only: any other key is refused"
    local key
    for key in size.cpu=1 pool.cpu=1 image=registry.example.test/x:1 runner.namespace=x client=x quota.pods=1 labels.a=b limits.pods=1; do
        t "an entry with the key ${key%%=*} is refused by the schema" render_fails -f "$EXAMPLE" --set "namespaces[0].${key}"
    done
}

check_the_cluster_scoped_objects_do_not_depend_on_the_number_of_namespaces() {
    section "one, four or seven namespaces give the same list of cluster-scoped objects of the operator part"
    t "the platform render, the example and the seven namespace render hold the same ClusterRoles, ClusterRoleBindings and definitions" test "$(cluster_objects platform)" = "$(cluster_objects keeper)" -a "$(cluster_objects keeper)" = "$(cluster_objects rights_five)"
    t "the list is not empty" test -n "$(cluster_objects platform)"
}
