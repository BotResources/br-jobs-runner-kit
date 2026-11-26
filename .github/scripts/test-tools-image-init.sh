#!/usr/bin/env bash

INIT_CHECKS='
for p in bsdtar pause; do
  eq "$p mode" "$(stat -c %a /opt/sbx/bin/$p)" 555
  eq "$p owner" "$(stat -c %u:%g /opt/sbx/bin/$p)" "$EXPECTED_OWNER"
done
eq "bin directory mode" "$(stat -c %a /opt/sbx/bin)" 755
eq "VERSION mode" "$(stat -c %a /opt/sbx/VERSION)" 444
eq "top-level entries" "$(ls -A /opt/sbx | tr "\n" " ")" "VERSION bin "
eq "bin entries" "$(ls -A /opt/sbx/bin | tr "\n" " ")" "bsdtar pause "
first=$(/opt/sbx/bin/bsdtar --version | head -n 1)
has "bsdtar runs in the second image" "$first" "bsdtar 3.8"
timeout 1 /opt/sbx/bin/pause
eq "pause runs in the second image and keeps waiting" "$?" 124
out=$(touch /opt/sbx/x 2>&1); rc=$?
ne0 "a write on the read-only mount fails" "$rc"
has "the write fails because the mount is read-only" "$out" "Read-only file system"
'

init_extraction() {
  tools_run "$@" -v "${INIT_VOLUME}:/opt/sbx" "$IMAGE" /sbx/bin/bsdtar $INIT_FLAGS -C /opt/sbx 2>&1
}

test_2_body() {
  local out code
  INIT_VOLUME="$TOOLS_VOL"
  docker volume create "$TOOLS_VOL" >/dev/null
  out=$(init_extraction $SEVEN_CAPS)
  code=$?
  eq "first init extraction exit" "$code" 0
  eq "first init extraction output" "$out" ""
  out=$(init_extraction $SEVEN_CAPS)
  code=$?
  eq "second init extraction exit" "$code" 0
  eq "second init extraction output" "$out" ""
  probe_checks "EXPECTED_OWNER=0:0
${INIT_CHECKS}"
}

test_2_init_extraction() {
  run_host_test 2 "init extraction as root with the 7 capabilities, run twice; read-only mount in a second image" test_2_body
}

NON_ROOT_USER=10001:10001
NON_ROOT_CAPS="--cap-drop ALL --security-opt no-new-privileges"

test_12_body() {
  local volume="${RUN_ID}-user-tools" control="${RUN_ID}-control-tools" out code owner
  owner="${NON_ROOT_USER}"
  docker volume create "$volume" >/dev/null
  docker volume create "$control" >/dev/null
  docker run --rm --platform "$PLATFORM" -v "${volume}:/v" -v "${control}:/c" "$PROBE_IMAGE" chown "$NON_ROOT_USER" /v /c
  INIT_VOLUME="$volume"
  out=$(init_extraction --user "$NON_ROOT_USER" $NON_ROOT_CAPS)
  code=$?
  eq "first init extraction as uid 10001, no capability: exit" "$code" 0
  eq "first init extraction as uid 10001, no capability: output" "$out" ""
  out=$(init_extraction --user "$NON_ROOT_USER" $NON_ROOT_CAPS)
  code=$?
  eq "second init extraction as uid 10001, no capability: exit" "$code" 0
  eq "second init extraction as uid 10001, no capability: output" "$out" ""
  TOOLS_VOL_SAVED="$TOOLS_VOL"
  TOOLS_VOL="$volume"
  probe_checks "EXPECTED_OWNER=${owner}
${INIT_CHECKS}"
  TOOLS_VOL="$TOOLS_VOL_SAVED"

  python3 -I - "${WORK_DIR}/closed-bin.tar" <<'PY'
import io, sys, tarfile
with tarfile.open(sys.argv[1], "w", format=tarfile.PAX_FORMAT) as t:
    directory = tarfile.TarInfo("bin")
    directory.type, directory.mode = tarfile.DIRTYPE, 0o555
    t.addfile(directory)
    body = b"x"
    member = tarfile.TarInfo("bin/pause")
    member.size, member.mode = len(body), 0o555
    t.addfile(member, io.BytesIO(body))
PY
  INIT_VOLUME="$control"
  out=$(docker run --rm -i --platform "$PLATFORM" --user "$NON_ROOT_USER" $NON_ROOT_CAPS -v "${control}:/opt/sbx" "$IMAGE" \
    /sbx/bin/bsdtar -x -p --no-same-owner -f - -C /opt/sbx <"${WORK_DIR}/closed-bin.tar" 2>&1)
  eq "control, first extraction of a tar with bin 0555: exit" "$?" 0
  out=$(docker run --rm -i --platform "$PLATFORM" --user "$NON_ROOT_USER" $NON_ROOT_CAPS -v "${control}:/opt/sbx" "$IMAGE" \
    /sbx/bin/bsdtar -x -p --no-same-owner -f - -C /opt/sbx <"${WORK_DIR}/closed-bin.tar" 2>&1)
  code=$?
  eq "control, second extraction of a tar with bin 0555: exit" "$code" 1
  has "control, second extraction cannot replace the files of bin" "$out" "Permission denied"
}

test_12_init_extraction_as_sandbox_user() {
  run_host_test 12 "init extraction as uid 10001, no capability, no new privileges, on a volume the user owns; second run; control with a closed bin" test_12_body
}

test_2b_body() {
  local remount='
out=$(mount -o remount,rw /opt/sbx 2>&1); rc=$?
mount_options() { local point options; while read -r _ _ _ _ point options _; do [ "$point" = /opt/sbx ] && echo "$options"; done </proc/self/mountinfo; }
'
  local refused="${remount}"'
eq "user" "$(id -u)" 0
ne0 "root without CAP_SYS_ADMIN: the remount of the tools mount read-write fails" "$rc"
case "$out" in
  *"ermission denied"* | *"must be superuser"*) pass "the remount is refused for lack of permission" ;;
  *) fail "the remount failed for another reason: [$out]" ;;
esac
case ",$(mount_options)," in
  *,ro,*) pass "the tools mount is still read-only" ;;
  *) fail "the tools mount options: $(mount_options)" ;;
esac
out=$(touch /opt/sbx/x 2>&1); rc=$?
ne0 "a write on the tools mount still fails" "$rc"
has "the write fails because the mount is read-only" "$out" "Read-only file system"
'
  local control="${remount}"'
eq "control, the remount as a privileged root: exit" "$rc" 0
case ",$(mount_options)," in
  *,rw,*) pass "control, the mount is read-write after the remount" ;;
  *) fail "control, the mount options: $(mount_options)" ;;
esac
'
  probe_checks "$refused" --user 0 $SEVEN_CAPS
  probe_checks "$control" --user 0 --privileged
}

test_2b_remount_refused() {
  run_host_test 2b "root with the 7 capabilities cannot remount the read-only tools mount; a privileged control can" test_2b_body
}
