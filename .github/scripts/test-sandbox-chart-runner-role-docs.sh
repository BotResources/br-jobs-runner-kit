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
    t "the README says that the two verbs give the same act" grep -qF 'The two verbs give the same act: to start a command in a pod of that namespace.' <<<"$(runner_role_paragraph)"
    t "the README says that a POST request needs create" grep -qF 'A client that opens the exec stream with a POST request (SPDY) needs `create`.' <<<"$(runner_role_paragraph)"
    t "the README says that a WebSocket sends a GET request, asks get, and asks create as well from 1.35 with the feature gate on, and get only before" grep -qF 'A client that opens it with a WebSocket sends a GET request, so the API server asks for `get`; from Kubernetes 1.35 it asks for `create` as well, while the feature gate `AuthorizePodWebsocketUpgradeCreatePermission` is on (beta in 1.35, on by default), and before 1.35 it asked for `get` only.' <<<"$(runner_role_paragraph)"
    t "the README says that the adapter uses a WebSocket and needs both verbs from 1.35" grep -qF 'The adapter uses a WebSocket, so on Kubernetes 1.35 or later it needs both verbs, and before 1.35 `get` is enough.' <<<"$(runner_role_paragraph)"
    t "the README says that get on pods/exec adds no read of pods, pods/log or secrets" grep -qF 'The verb `get` in the `pods/exec` rule adds no read of `pods`, of `pods/log` or of `secrets`: the API server checks each resource and each subresource by its own name.' <<<"$(runner_role_paragraph)"
    t "the CHANGELOG 0.3.0 says that the runner Role gains get on pods/exec" test "$(runner_role_changelog | grep -cF 'the runner Role `br-sandbox-runner` of each sandbox namespace gains `get` on `pods/exec`')" -eq 1
    t "the CHANGELOG 0.3.0 says what 0.2.x rendered and what 0.3.0 renders" test "$(runner_role_changelog | grep -cF 'Versions 0.2.x rendered `create` only on `pods/exec`; 0.3.0 renders `create` and `get`.')" -eq 1
    t "the CHANGELOG 0.3.0 says that an upgrade grants one more verb" test "$(runner_role_changelog | grep -cF 'An upgrade from 0.2.x so grants one more verb to the runner service account.')" -eq 1
    t "the CHANGELOG 0.3.0 says that no other right changes" test "$(runner_role_changelog | grep -cF 'Every other rule of the runner Role is unchanged, and no other Role, ClusterRole, binding or admission rule changes.')" -eq 1
    t "the CHANGELOG 0.2.4 entry is untouched: it still says that the Role is not changed" test "$(awk '/^## 0.2.4$/ {on=1; next} /^## / {on=0} on' "$CHART/CHANGELOG.md" | grep -cF 'and the Role is not changed')" -eq 1
}
