#!/usr/bin/env bash
set -uo pipefail

IMAGE="${1:?usage: test-tools-image.sh <image-reference> <platform>}"
PLATFORM="${2:?usage: test-tools-image.sh <image-reference> <platform>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROBE_IMAGE="${PROBE_IMAGE:-debian:bookworm-slim@sha256:e67eeead736ff7bbe258ef088bff683635c561b1fcfefad0806c1df4ab49ce0e}"
RUN_ID="sbxt-$$-$(date +%s)"
TOOLS_VOL="${RUN_ID}-tools"
DATA_VOL="${RUN_ID}-data"
WORK_DIR="$(mktemp -d)"
RESULTS_FILE="${RESULTS_FILE:-${WORK_DIR}/results.txt}"
SEVEN_CAPS="--cap-drop ALL --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER --cap-add FSETID --cap-add SETUID --cap-add SETGID --cap-add KILL"
export SEVEN_CAPS IMAGE PLATFORM PROBE_IMAGE RUN_ID TOOLS_VOL DATA_VOL WORK_DIR RESULTS_FILE

cleanup() {
  docker ps -aq --filter "name=${RUN_ID}" | xargs -r docker rm -f >/dev/null 2>&1
  docker volume ls -q --filter "name=${RUN_ID}" | xargs -r docker volume rm >/dev/null 2>&1
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

source "${SCRIPT_DIR}/test-tools-image-lib.sh"
source "${SCRIPT_DIR}/test-tools-image-build.sh"
source "${SCRIPT_DIR}/test-tools-image-tar.sh"
source "${SCRIPT_DIR}/test-tools-image-pause.sh"
source "${SCRIPT_DIR}/test-tools-image-roles.sh"

docker pull --quiet --platform "$PLATFORM" "$PROBE_IMAGE" >/dev/null
docker volume create "$DATA_VOL" >/dev/null
echo "image=${IMAGE} platform=${PLATFORM} probe=${PROBE_IMAGE} load=$(uptime | sed 's/.*load averages*: //')"

test_1_static_and_version
test_1b_image_size
test_2_init_extraction
test_3_n15_round_trip
test_4_capability_set
test_5_end_of_input
test_6_names
test_8_special_entries
test_7_pause_as_pid_1
test_9_non_root_same_owner
test_10_read_only_root_second_container
test_10b_save_read_side_as_non_root

echo "failed tests: ${FAILED_TESTS}"
[ "$FAILED_TESTS" -eq 0 ]
