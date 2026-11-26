#!/usr/bin/env bash
set -uo pipefail

IMAGE="${1:?usage: test-tools-image.sh <image-reference> <platform>}"
PLATFORM="${2:?usage: test-tools-image.sh <image-reference> <platform>}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
README="${SCRIPT_DIR}/../../images/br-sandbox-tools/README.md"
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

SAVE_FLAGS=$(readme_flags "$README" 's/^| save | `bsdtar \(.*\) -C <root> -- <path>\.\.\.` |$/\1/p') || exit 1
RESTORE_FLAGS=$(readme_flags "$README" 's/^| restore and copy-in[^|]*| `bsdtar \(.*\) -C <dir>` |$/\1/p') || exit 1
INIT_FLAGS=$(readme_flags "$README" 's#^/sbx/bin/bsdtar \(.*\) -C <mountPath>$#\1#p') || exit 1
export SAVE_FLAGS RESTORE_FLAGS INIT_FLAGS

source "${SCRIPT_DIR}/test-tools-image-build.sh"
source "${SCRIPT_DIR}/test-tools-image-content.sh"
source "${SCRIPT_DIR}/test-tools-image-init.sh"
source "${SCRIPT_DIR}/test-tools-image-tar.sh"
source "${SCRIPT_DIR}/test-tools-image-xattr.sh"
source "${SCRIPT_DIR}/test-tools-image-stream.sh"
source "${SCRIPT_DIR}/test-tools-image-names.sh"
source "${SCRIPT_DIR}/test-tools-image-pause.sh"
source "${SCRIPT_DIR}/test-tools-image-roles.sh"

docker pull --quiet --platform "$PLATFORM" "$PROBE_IMAGE" >/dev/null
docker volume create "$DATA_VOL" >/dev/null
echo "image=${IMAGE} platform=${PLATFORM} probe=${PROBE_IMAGE}"
echo "save flags:    ${SAVE_FLAGS}"
echo "restore flags: ${RESTORE_FLAGS}"
echo "init flags:    ${INIT_FLAGS}"

test_1_static_and_version
test_1b_image_size
test_11_image_content
test_2_init_extraction
test_12_init_extraction_as_sandbox_user
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
