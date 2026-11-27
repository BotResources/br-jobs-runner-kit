#!/usr/bin/env bash

runner_subject_of() {
    jqv "$1" ".[] | select(.kind == \"RoleBinding\" and .metadata.name == \"br-sandbox-runner\" and .metadata.namespace == \"$2\") | .subjects[0] | .namespace + \"/\" + .name"
}

client_of() {
    adapter_doc "$1" "$2" '.client_id'
}

check_the_scope_table_has_no_overlap() {
    section "the scope table: no scope prefixes another, every namespace has one scope"
    fails_with "BR_SANDBOX_SCOPES_OVERLAP{scope=sbx,other=sbx-alpha}" -f "$(vals overlap '.scopes.sbx = {"client_id": "all", "runner": {"service_account": "runner", "namespace": "runners"}}')"
    t "a scope that shares letters with another but not the prefix with a dash is not an overlap" render notprefix -f "$(vals notprefix '.scopes.sbx-alph = {"client_id": "other", "runner": {"service_account": "runner", "namespace": "runners"}}')"
    fails_with "BR_SANDBOX_NO_SCOPE{namespace=other-x-none}" -f "$(vals noscope '.namespaces += [{"name": "other-x-none", "mode": "none"}]')"
    fails_with "BR_SANDBOX_NO_SCOPE{namespace=sbx-alpha-none}" -f "$(vals emptyscopes '.scopes = {}')"
    fails_with "BR_SANDBOX_NO_SCOPE{namespace=sbx-alphabet-none}" -f "$(vals dashless '.namespaces += [{"name": "sbx-alphabet-none", "mode": "none"}]')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=scopes.sbx-alpha.runner.service_account}" -f "$(vals norunnersa 'del(.scopes.sbx-alpha.runner.service_account)')"
    fails_with "BR_SANDBOX_MISSING_VALUE{key=scopes.sbx-alpha.runner.namespace}" -f "$(vals norunnerns 'del(.scopes.sbx-alpha.runner.namespace)')"
    t "a namespace named like its scope matches it" render equalname -f "$(vals equalname '.namespaces += [{"name": "sbx-alpha", "mode": "none"}]')"
}

check_client_and_runner_are_the_values_of_the_scope() {
    section "client_id and the runner subject are the values of the scope that matches the name"
    local row namespace client runner
    for row in 'sbx-alpha-none|alpha|runners/runner' 'sbx-alpha-allow|alpha|runners/runner' 'sbx-alpha-full|alpha|runners/runner' 'sbx-beta-none|beta|runners/runner'; do
        IFS='|' read -r namespace client runner <<<"$row"
        t "${namespace}: client_id is ${client}" test "$(client_of ex "$namespace")" = "$client"
        t "${namespace}: the runner binding names ${runner}" test "$(runner_subject_of ex "$namespace")" = "$runner"
    done
    render scoped -f "$(vals scoped '.scopes.sbx-beta.client_id = "beta-saves" | .scopes.sbx-beta.runner = {"service_account": "beta-runner", "namespace": "beta-runners"}')"
    t "a client_id of another value reaches the adapter document of its namespace only" test "$(client_of scoped sbx-beta-none)" = beta-saves -a "$(client_of scoped sbx-alpha-none)" = alpha
    t "a runner of another value reaches the binding of its namespace only" test "$(runner_subject_of scoped sbx-beta-none)" = beta-runners/beta-runner -a "$(runner_subject_of scoped sbx-alpha-none)" = runners/runner
    render defaulted -f "$(vals defaulted 'del(.scopes.sbx-beta.client_id)')"
    t "a scope without client_id gives its own name as client_id" test "$(client_of defaulted sbx-beta-none)" = sbx-beta
    render sharedscope -f "$(vals sharedscope '.namespaces += [{"name": "sbx-alpha-extra-none", "mode": "none"}]')"
    t "two namespaces of one scope share the client_id and the runner" test "$(client_of sharedscope sbx-alpha-extra-none)" = alpha -a "$(runner_subject_of sharedscope sbx-alpha-extra-none)" = runners/runner
}

check_no_label_and_no_entry_key_gives_a_scope() {
    section "no label and no key of an entry gives a client_id or a runner"
    local key
    for key in client client_id runner labels scope; do
        fails_with "values don't meet the specifications" -f "$EXAMPLE" --set "namespaces[0].${key}=evil"
    done
    t "no template reads a label of a namespace to find a client or a runner" test -z "$(grep -rnE 'lookup |\$ns\.labels|\$entry\.labels' "$CHART/templates" || true)"
    t "no managed namespace carries a client label" jqt ex '[.[] | select(.kind == "Namespace" and .metadata.labels["br.sandbox/managed"] == "true") | .metadata.labels | has("br.sandbox/client")] | all(. == false)'
}
