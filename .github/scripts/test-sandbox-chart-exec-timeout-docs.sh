#!/usr/bin/env bash

exec_timeout_changelog() {
    awk '/^## 0.3.0$/ {on=1; next} /^## / {on=0} on' "$CHART/CHANGELOG.md"
}

adapter_paragraph() {
    grep '^\*\*Adapter document\.\*\*' "$CHART/README.md"
}

check_the_docs_state_the_exec_timeout() {
    section "the README and the CHANGELOG state what exec_timeout_seconds is, its default and what it refuses"
    t "the chart is at version 0.3.0" grep -qx 'version: 0.3.0' "$CHART/Chart.yaml"
    t "the CHANGELOG has the heading 0.3.0 once" test "$(grep -cx '## 0.3.0' "$CHART/CHANGELOG.md")" -eq 1
    t "the app version is 0.3.0 too" grep -qx 'appVersion: "0.3.0"' "$CHART/Chart.yaml"
    t "the 0.2.4 heading is still in the CHANGELOG" test "$(grep -cx '## 0.2.4' "$CHART/CHANGELOG.md")" -eq 1
    t "the README values table has the row with the default 600 and the bound of one run in the foreground, marked as read from the code and not tested" grep -qF '| `exec_timeout_seconds` | `600` | An integer above zero. The key `exec_timeout_seconds` of the adapter document: the bound, in seconds, of one `run` in the foreground; the same key bounds the seed copy at an open. A command that needs longer uses the background form of `run`, which returns the id of the process and not its output. What the key bounds, and what the background form returns, are read from the code of the adapter and are not tested here or on a cluster.' "$CHART/README.md"
    t "the README row says that 600 is the choice of the chart, ten minutes" grep -qF 'The default `600` (ten minutes) is the choice of the chart.' "$CHART/README.md"
    t "the README row names the two refusals" grep -qF 'is refused by the schema (`0`, a negative number, a string, a float) or by `BR_SANDBOX_EXEC_TIMEOUT_INVALID{key=exec_timeout_seconds}` (`null`, which removes the key)' "$CHART/README.md"
    t "the README lists the key among the keys of the adapter document" grep -qF '`restore_timeout_seconds`, `exec_timeout_seconds`, `idle_ttl_seconds` (`idle_ttl`)' "$CHART/README.md"
    t "the README says that the key bounds the foreground run and the seed copy, read from the adapter code and not tested here or on a cluster" grep -qF 'The key `exec_timeout_seconds` is the bound of one `run` in the foreground, and the same key bounds the seed copy at an open; a command that needs longer uses the background form of `run`, which returns the id of the process and not its output. This is read from the code of the adapter and is not tested here or on a cluster.' <<<"$(adapter_paragraph)"
    t "the README says that before 0.3.0 the document had no such key and that the adapter applied no bound, read from its code" grep -qF 'Before 0.3.0 the document had no such key, and the adapter, read from its code, then applied no bound to these two operations.' <<<"$(adapter_paragraph)"
    t "the README no longer lists exec_timeout_seconds among the keys with no chart value" grep -qF 'The chart has no value for `save_zstd_level`, `open_timeout_seconds`, `save_timeout_seconds`, `claim_max_attempts` and `cold_create`' <<<"$(adapter_paragraph)"
    t "the README render faults name BR_SANDBOX_EXEC_TIMEOUT_INVALID" grep -qF '`BR_SANDBOX_EXEC_TIMEOUT_INVALID{key=exec_timeout_seconds}` (a null `exec_timeout_seconds`; `0`, a negative number, a string and a float are refused by the schema)' "$CHART/README.md"
    t "the CHANGELOG 0.3.0 says that the document gains one key and every document holds it" test "$(exec_timeout_changelog | grep -cF 'The adapter document (the ConfigMap `br-sandbox-config` of each sandbox namespace, key `config.json`) gains one key, `exec_timeout_seconds`, and every document holds it.')" -eq 1
    t "the CHANGELOG 0.3.0 says what the key bounds, read from the adapter code and not tested here or on a cluster" test "$(exec_timeout_changelog | grep -cF 'The key `exec_timeout_seconds` is the bound, in seconds, of one `run` in the foreground; the same key bounds the seed copy at an open. A command that needs longer uses the background form of `run`, which returns the id of the process and not its output. This is read from the code of the adapter and is not tested here or on a cluster.')" -eq 1
    t "the CHANGELOG 0.3.0 says that foreground commands with no bound under 0.2.x are cut after 600 seconds by default and that the key can be raised" test "$(exec_timeout_changelog | grep -cF 'Effect of an upgrade from 0.2.x: foreground commands that ran with no bound under 0.2.x (read from the code of the adapter, as above) are cut after 600 seconds by default; a runner that needs longer raises `exec_timeout_seconds`.')" -eq 1
    t "the CHANGELOG 0.3.0 says that 600 is the choice of the chart" test "$(exec_timeout_changelog | grep -cF 'The default `600` (ten minutes) is the choice of the chart.')" -eq 1
    t "the CHANGELOG 0.3.0 says what the document held before and what the adapter did" test "$(exec_timeout_changelog | grep -cF 'Before 0.3.0 the document had no such key, and the adapter, read from its code, then applied no bound to these two operations.')" -eq 1
    t "the CHANGELOG 0.3.0 names the refusals" test "$(exec_timeout_changelog | grep -cF '`0`, a negative number, a string and a float by the schema, `null` (which removes the key) by the new render fault `BR_SANDBOX_EXEC_TIMEOUT_INVALID{key=exec_timeout_seconds}`')" -eq 1
    t "the CHANGELOG 0.3.0 says that the lint loop covers the new overlay" test "$(exec_timeout_changelog | grep -cF 'the lint loop of the CI covers the new overlay `ci/exec-timeout-values.yaml`')" -eq 1
    t "the 0.2.4 entry still says that the chart had no value for exec_timeout_seconds" test "$(awk '/^## 0.2.4$/ {on=1; next} /^## / {on=0} on' "$CHART/CHANGELOG.md" | grep -cF 'the keys that no chart value feeds (`save_zstd_level`, `open_timeout_seconds`, `exec_timeout_seconds`')" -eq 1
}
