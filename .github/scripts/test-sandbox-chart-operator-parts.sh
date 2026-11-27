#!/usr/bin/env bash

check_a_keeper_key_is_a_named_render_fault() {
    section "the keeper values are removed: a keeper key is a named render fault"
    fails_with "BR_SANDBOX_KEEPER_VALUES_REMOVED{key=keeper}" -f "$EXAMPLE" --set keeper.enabled=true
    fails_with "BR_SANDBOX_KEEPER_VALUES_REMOVED{key=keeper}" -f "$EXAMPLE" -f "$(vals keeperkey '.keeper = {"enabled": false}')"
}
