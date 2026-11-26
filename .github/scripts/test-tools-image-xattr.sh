#!/usr/bin/env bash

make_xattr_tar() {
  python3 -I - "${WORK_DIR}/xattr.tar" <<'PY'
import io, sys, tarfile
with tarfile.open(sys.argv[1], "w", format=tarfile.PAX_FORMAT) as t:
    body = b"hello\n"
    info = tarfile.TarInfo("w/with_xattr.txt")
    info.size = len(body)
    info.mode = 0o644
    info.uid = 1000
    info.gid = 1000
    info.pax_headers = {"SCHILY.xattr.security.sbxtest": "1", "SCHILY.xattr.user.sbxtest": "1"}
    t.addfile(info, io.BytesIO(body))
PY
}

without_no_flags() {
  echo "${RESTORE_FLAGS% --no-xattrs --no-acls --no-fflags}"
}

extract_xattr_tar() {
  local flags="$1"
  probe_run -i $SEVEN_CAPS "$PROBE_IMAGE" bash -c 'mkdir -p /o && /opt/sbx/bin/bsdtar "$@" -C /o 2>/tmp/err; echo "rc=$?"; cat /tmp/err; ls -A /o/w 2>&1 | sed "s/^/written: /"' _ $flags <"${WORK_DIR}/xattr.tar" 2>&1
}

test_4b_body() {
  local default_flags out
  default_flags=$(without_no_flags)
  if [ "$default_flags" = "$RESTORE_FLAGS" ]; then fail "the README restore flags end with --no-xattrs --no-acls --no-fflags"; else pass "the default flags are the README flags without --no-xattrs --no-acls --no-fflags"; fi
  out=$(extract_xattr_tar "$default_flags")
  eq "exit" "$(sed -n 's/^rc=//p' <<<"$out")" 1
  has "message" "$out" "Cannot restore extended attributes: security.sbxtest"
  has "the file is written" "$out" "written: with_xattr.txt"
}

test_4c_body() {
  local out
  out=$(extract_xattr_tar "$RESTORE_FLAGS")
  eq "exit" "$(sed -n 's/^rc=//p' <<<"$out")" 0
  eq "output" "$out" $'rc=0\nwritten: with_xattr.txt'
}

test_4d_body() {
  local caps="--security-opt seccomp=unconfined --security-opt apparmor=unconfined $SEVEN_CAPS --cap-add SYS_ADMIN --cap-add SETFCAP"
  local can out
  can=$(probe_run $caps "$PROBE_IMAGE" unshare --user --map-root-user true 2>&1; echo "rc=$?")
  if [ "$(tail -n 1 <<<"$can")" != "rc=0" ]; then
    if daemon_runs_platform_natively; then
      fail "a user namespace cannot be created on a native platform: ${can//$'\n'/ }"
    else
      skip "the emulated platform cannot create a user namespace: ${can//$'\n'/ }"
    fi
    return
  fi
  out=$(probe_run -i $caps "$PROBE_IMAGE" \
    unshare --user --map-root-user bash -c 'mkdir -p /tmp/o; /opt/sbx/bin/bsdtar "$@" -C /tmp/o 2>/tmp/err; echo "rc=$?"; cat /tmp/err; ls -A /tmp/o/w 2>&1 | sed "s/^/written: /"' _ $RESTORE_FLAGS <"${WORK_DIR}/xattr.tar" 2>&1)
  eq "exit" "$(sed -n 's/^rc=//p' <<<"$out")" 1
  has "message" "$out" "Can't set user=1000/group=1000"
  has "message reason" "$out" "Invalid argument"
  has "the file is written" "$out" "written: with_xattr.txt"
}
