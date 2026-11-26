#!/usr/bin/env bash

EXPECTED_TEST_IDS="1 1b 11 2 2b 12 3 4a 4b 4c 4d 5 6 8 7 7b 9 10 10b 10c 13"

check_test_ids() {
  local results="$1" id missing="" unlisted="" ran
  for id in $EXPECTED_TEST_IDS; do
    if ! grep -Eq "^RESULT ${id} (PASS|FAIL|SKIP) " "$results" 2>/dev/null; then
      missing="${missing} ${id}"
    fi
  done
  if [ -f "$results" ]; then
    for ran in $(sed -n 's/^RESULT \([^ ]*\) .*/\1/p' "$results" | sort -u); do
      case " ${EXPECTED_TEST_IDS} " in
        *" ${ran} "*) ;;
        *) unlisted="${unlisted} ${ran}" ;;
      esac
    done
  fi
  if [ -n "$missing" ]; then
    echo "tests that did not run:${missing}" >&2
  fi
  if [ -n "$unlisted" ]; then
    echo "tests that ran and are not in EXPECTED_TEST_IDS:${unlisted}" >&2
  fi
  [ -z "${missing}${unlisted}" ]
}

exit_gate() {
  local status=$?
  if ! check_test_ids "$RESULTS_FILE"; then
    status=1
  fi
  if declare -F cleanup >/dev/null; then
    cleanup
  fi
  exit "$status"
}
