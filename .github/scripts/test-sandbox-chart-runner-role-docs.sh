#!/usr/bin/env bash

runner_role_changelog() {
    awk '/^## 0.3.0$/ {on=1; next} /^## / {on=0} on' "$CHART/CHANGELOG.md"
}

runner_role_paragraph() {
    awk '/^## Runner role$/ {on=1; next} /^## / {on=0} on' "$CHART/README.md"
}

check_the_docs_state_the_runner_role() {
    section "the README and the CHANGELOG state the rights of the runner Role on pods/exec"
    t "the README Role description lists pods/exec with get and create" grep -qF '`pods/exec` get and create; `replicasets` get, list, watch' <<<"$(runner_role_paragraph)"
    t "the README Role description no longer lists pods/exec with create alone" test "$(runner_role_paragraph | grep -cF '`pods/exec` create;')" -eq 0
    t "the README says that the Role holds get and create on pods/exec in each sandbox namespace" grep -qF 'The runner Role holds `get` and `create` on `pods/exec` in each sandbox namespace.' <<<"$(runner_role_paragraph)"
    t "the README marks every Kubernetes sentence of the paragraph as read from public pages and not tested on a cluster" grep -qF 'Every sentence of this paragraph about the behaviour of Kubernetes is read from public pages and is not tested on a cluster.' <<<"$(runner_role_paragraph)"
    t "the README names the feature gates reference with its address and the gate" grep -qF 'the Kubernetes feature gates reference (https://kubernetes.io/docs/reference/command-line-tools-reference/feature-gates/, the gate `AuthorizePodWebsocketUpgradeCreatePermission`)' <<<"$(runner_role_paragraph)"
    t "the README names KEP 4006 with its address" grep -qF 'KEP 4006 of kubernetes/enhancements (https://github.com/kubernetes/enhancements/tree/master/keps/sig-api-machinery/4006-transition-spdy-to-websockets)' <<<"$(runner_role_paragraph)"
    t "the README names the issue 78741 with its address" grep -qF 'the issue kubernetes/kubernetes#78741 (https://github.com/kubernetes/kubernetes/issues/78741)' <<<"$(runner_role_paragraph)"
    t "the README names the RBAC reference with its address" grep -qF 'the Kubernetes RBAC reference (https://kubernetes.io/docs/reference/access-authn-authz/rbac/)' <<<"$(runner_role_paragraph)"
    t "the README says that a POST request needs create and that the WebSocket handshake is a GET request" grep -qF 'A client that opens the exec stream with a POST request (SPDY) needs `create`, and the WebSocket handshake is a GET request (feature gates reference, KEP 4006).' <<<"$(runner_role_paragraph)"
    t "the README says that, while the feature gate is on, a WebSocket upgrade must also be authorized to create, beta since 1.35, on by default" grep -qF 'while the feature gate is on, a WebSocket upgrade on `pods/exec`, `pods/attach` and `pods/portforward` must also be authorized to `create` the subresource; the feature gates reference lists the gate as beta since 1.35, on by default.' <<<"$(runner_role_paragraph)"
    t "the README says who states that get was enough, and that the version boundary is an inference" grep -qF 'that `get` alone was enough on every Kubernetes before 1.35 is our inference from these pages, not a sentence of them.' <<<"$(runner_role_paragraph)"
    t "the README says that the issue 78741 states that a WebSocket exec was allowed with get" grep -qF 'The issue kubernetes/kubernetes#78741 says that a WebSocket exec was allowed with `get` on `pods/exec`' <<<"$(runner_role_paragraph)"
    t "the README says that the adapter uses a WebSocket, read from the adapter code and not tested on a cluster" grep -qF 'The adapter uses a WebSocket: this is read from the code of the adapter (`kube` with its `ws` feature), and not tested on a cluster.' <<<"$(runner_role_paragraph)"
    t "the README keeps the condition: both verbs on 1.35 or later while the feature gate is on, on by default from 1.35" grep -qF 'on Kubernetes 1.35 or later while the feature gate is on (it is on by default from 1.35), it needs `create` as well.' <<<"$(runner_role_paragraph)"
    t "the README states no GA or locked version of the feature gate" test "$(runner_role_paragraph | grep -ciE 'locked|[[:space:]]GA[[:space:]]|stable')" -eq 0
    t "the README says that get on pods/exec adds no read of pods, pods/log or secrets, and names the RBAC reference as the source" grep -qF 'The verb `get` in the `pods/exec` rule adds no read of `pods`, of `pods/log` or of `secrets`: the RBAC reference names each resource and each subresource separately (`pods` and `pods/log` in the example of that page).' <<<"$(runner_role_paragraph)"
    t "the CHANGELOG 0.3.0 says that the runner Role gains get on pods/exec" test "$(runner_role_changelog | grep -cF 'the runner Role `br-sandbox-runner` of each sandbox namespace gains `get` on `pods/exec`')" -eq 1
    t "the CHANGELOG 0.3.0 says what 0.2.x rendered and what 0.3.0 renders" test "$(runner_role_changelog | grep -cF 'Versions 0.2.x rendered `create` only on `pods/exec`; 0.3.0 renders `create` and `get`.')" -eq 1
    t "the CHANGELOG 0.3.0 says that an upgrade grants one more verb" test "$(runner_role_changelog | grep -cF 'An upgrade from 0.2.x so grants one more verb to the runner service account.')" -eq 1
    t "the CHANGELOG 0.3.0 says that no other right changes" test "$(runner_role_changelog | grep -cF 'Every other rule of the runner Role is unchanged, and no other Role, ClusterRole, binding or admission rule changes.')" -eq 1
    t "the CHANGELOG 0.3.0 marks the Kubernetes behaviour as read from public pages, not tested on a cluster, and keeps the condition on the feature gate" test "$(runner_role_changelog | grep -cF 'on Kubernetes 1.35 or later while the feature gate `AuthorizePodWebsocketUpgradeCreatePermission` is on (it is on by default from 1.35), `create` as well. The Kubernetes behaviour in this sentence is read from public pages (the Kubernetes feature gates reference, KEP 4006 of kubernetes/enhancements, the issue kubernetes/kubernetes#78741) and is not tested on a cluster')" -eq 1
    t "the CHANGELOG 0.2.4 entry is untouched: it still says that the Role is not changed" test "$(awk '/^## 0.2.4$/ {on=1; next} /^## / {on=0} on' "$CHART/CHANGELOG.md" | grep -cF 'and the Role is not changed')" -eq 1
}
