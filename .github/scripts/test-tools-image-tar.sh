#!/usr/bin/env bash

TREE_SCRIPT='
set -u
B=/opt/sbx/bin/bsdtar
CREATE="$B --create --file - --format pax --numeric-owner --no-xattrs --no-acls --no-fflags"
EXTRACT="$B --extract --file - --preserve-permissions --same-owner --numeric-owner --no-xattrs --no-acls --no-fflags"
src=/data/src; dst=/data/dst
rm -rf /data; mkdir -p $src/work $src/home/agent/.config $dst
cd $src/work
for n in $(seq 0 24); do mkdir -p dir$n; done
mkdir -p empty_a empty_b/nested_empty
chmod 750 empty_a; chown 1234:5678 empty_a
for i in $(seq 0 999); do
  f=dir$((i % 25))/file$i.dat
  printf "content %s\n" "$i" > "$f"
  case $((i % 5)) in 0) chmod 644 "$f";; 1) chmod 600 "$f";; 2) chmod 755 "$f";; 3) chmod 640 "$f";; 4) chmod 750 "$f";; esac
  chown $((1000 + i % 7)):$((2000 + i % 3)) "$f"
  touch -d @$((1600000000 + i * 37)) "$f"
done
head -c 52428800 /dev/urandom > big.bin
chmod 744 big.bin; chown 4242:4343 big.bin; touch -d @1650000000 big.bin
ln -s dir0/file0.dat link_relative
ln -s /etc/hostname link_absolute
ln -s does/not/exist link_dangling
ln -s dir1 link_to_dir
touch -d @1700000000.123456789 subsecond.txt
printf "#!/bin/sh\necho hi\n" > run.sh; chmod 755 run.sh
printf "{}" > $src/home/agent/.config/x.json; printf "export A=1\n" > $src/home/agent/.bashrc
chown -R 10001:10001 $src/home/agent
manifest() {
  for root in work home/agent; do
    (cd "$1/$root"
    find . -printf "$root/%p\t%y\t%m\t%U\t%G\t%l\n" | LC_ALL=C sort
    find . -type f -exec sha256sum {} + | LC_ALL=C sort -k2
    find . -printf "%p\t%Ts\n" | LC_ALL=C sort)
  done
  stat -c "subsecond %.9Y" "$1/work/subsecond.txt"
}
manifest $src > /data/before.txt
t0=$(date +%s%N)
$CREATE -C $src -- work home/agent > /data/save.tar 2>/data/create.err; create_rc=$?
t1=$(date +%s%N)
cat /data/save.tar | $EXTRACT -C $dst 2>/data/extract.err; extract_rc=$?
t2=$(date +%s%N)
manifest $dst > /data/after.txt
entries=$($B -t -f /data/save.tar | wc -l)
echo "INFO: entries=$entries tar_bytes=$(stat -c %s /data/save.tar) create_ms=$(( (t1 - t0) / 1000000 )) extract_ms=$(( (t2 - t1) / 1000000 ))"
echo "INFO: create_rc=$create_rc extract_rc=$extract_rc create_stderr_bytes=$(stat -c %s /data/create.err) extract_stderr_bytes=$(stat -c %s /data/extract.err)"
echo "EXTRACT_RC=$extract_rc"; echo "EXTRACT_STDERR_BYTES=$(stat -c %s /data/extract.err)"
[ $create_rc -eq 0 ] && [ $extract_rc -eq 0 ] || echo "FAIL: nonzero exit"
if diff /data/before.txt /data/after.txt > /data/diff.txt; then
  echo "INFO: manifest equal ($(wc -l < /data/before.txt) lines: mode, uid, gid, symlink target, file hash, mtime seconds, sub-second mtime)"
else
  echo "FAIL: manifest differs"; head -n 20 /data/diff.txt
fi
cat /data/create.err /data/extract.err | head -n 5
'

test_3_n15_round_trip() {
  run_scripted_test 3 "N15 round trip: 1000 files, 50 MB file, symlinks, empty dirs, modes, uid/gid, mtime" \
    "$TREE_SCRIPT" $SEVEN_CAPS
  EXTRACT_RC=$(sed -n 's/^EXTRACT_RC=//p' "${WORK_DIR}/last_output")
  EXTRACT_STDERR_BYTES=$(sed -n 's/^EXTRACT_STDERR_BYTES=//p' "${WORK_DIR}/last_output")
}

test_4_capability_set() {
  if [ "${EXTRACT_RC:-x}" = 0 ] && [ "${EXTRACT_STDERR_BYTES:-x}" = 0 ]; then
    record PASS 4a "extraction as root with the 7-capability set: exit 0, no warning" "same-owner restore of test 3"
  else
    record FAIL 4a "extraction as root with the 7-capability set: exit 0, no warning" "exit=${EXTRACT_RC:-unknown} stderr_bytes=${EXTRACT_STDERR_BYTES:-unknown}"
  fi
  local tar_file="${WORK_DIR}/xattr.tar"
  python3 -I - "$tar_file" <<'PY'
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
  local flags_default=(--extract --file - --preserve-permissions --same-owner --numeric-owner)
  local flags_off=("${flags_default[@]}" --no-xattrs --no-acls --no-fflags)
  local out
  out=$(probe_run -i $SEVEN_CAPS "$PROBE_IMAGE" bash -c 'mkdir -p /o && /opt/sbx/bin/bsdtar "$@" -C /o; echo "rc=$?"' _ "${flags_default[@]}" <"$tar_file" 2>&1)
  record INFO 4b "open fault 4: security.* and user.* xattr, default flags, 7 caps" "$(tr '\n' ' ' <<<"$out")"
  out=$(probe_run -i $SEVEN_CAPS "$PROBE_IMAGE" bash -c 'mkdir -p /o && /opt/sbx/bin/bsdtar "$@" -C /o; echo "rc=$?"' _ "${flags_off[@]}" <"$tar_file" 2>&1)
  if [[ "$out" == *"rc=0"* ]] && [ "$(wc -l <<<"$out")" -eq 1 ]; then
    record PASS 4c "xattr entries with --no-xattrs --no-acls --no-fflags: exit 0, no warning" "$(tr '\n' ' ' <<<"$out")"
  else
    record FAIL 4c "xattr entries with --no-xattrs --no-acls --no-fflags: exit 0, no warning" "$(tr '\n' ' ' <<<"$out")"
  fi
  out=$(probe_run -i --security-opt seccomp=unconfined --security-opt apparmor=unconfined $SEVEN_CAPS --cap-add SYS_ADMIN --cap-add SETFCAP "$PROBE_IMAGE" \
    unshare --user --map-root-user bash -c 'mkdir -p /tmp/o; /opt/sbx/bin/bsdtar "$@" -C /tmp/o; echo "rc=$?"; ls -ln /tmp/o/w 2>&1' _ "${flags_off[@]}" <"$tar_file" 2>&1)
  record INFO 4d "open fault 4: chown to an id unmapped in the user namespace (unshare --map-root-user, uid 1000 in the tar)" "$(tr '\n' ' ' <<<"$out")"
}

test_5_end_of_input() {
  local script='
set -u
B=/opt/sbx/bin/bsdtar
mkdir -p /data/in /data/out && cd /data/in
head -c 1048576 /dev/urandom > one.bin; printf "x" > two.txt
$B --create --file /data/full.tar --format pax --numeric-owner -C /data/in -- one.bin two.txt
size=$(stat -c %s /data/full.tar)
s=$(date +%s%N)
{ cat /data/full.tar; sleep 10; } | $B -x -f - -C /data/out 2>/data/e1; rc=$?
e=$(date +%s%N)
ms=$(( (e - s) / 1000000 ))
echo "INFO: writer keeps pipe open 10 s after a complete tar: bsdtar exit=$rc after ${ms} ms"
if [ $ms -ge 9000 ]; then echo "INFO: bsdtar WAITS for end of input"; else echo "INFO: bsdtar STOPS at the end-of-archive blocks"; fi
s=$(date +%s%N)
{ head -c $((size / 2)) /data/full.tar; sleep 3; } | $B -x -f - -C /data/out 2>/data/e2; rc=$?
e=$(date +%s%N)
echo "INFO: truncated mid-file, pipe open 3 s: exit=$rc after $(( (e - s) / 1000000 )) ms stderr=$(head -c 200 /data/e2 | tr "\n" " ")"
[ $rc -ne 0 ] || echo "FAIL: truncated tar exits 0 with the pipe held open"
head -c $((size / 2)) /data/full.tar | $B -x -f - -C /data/out 2>/data/e3; rc=$?
echo "INFO: truncated mid-file, pipe closed: exit=$rc stderr=$(head -c 200 /data/e3 | tr "\n" " ")"
[ $rc -ne 0 ] || echo "FAIL: truncated tar exits 0"
head -c $((size - 1024)) /data/full.tar | $B -x -f - -C /data/out 2>/data/e4; rc=$?
echo "INFO: complete tar without the end-of-archive blocks, pipe closed: exit=$rc"
: | $B -x -f - -C /data/out 2>/data/e5; rc=$?
echo "INFO: empty input: exit=$rc stderr=$(head -c 200 /data/e5 | tr "\n" " ")"
'
  run_scripted_test 5 "open fault 2: bsdtar -x -f - end of input; truncated tar" "$script" $SEVEN_CAPS
}

test_6_names() {
  local script='
set -u
B=/opt/sbx/bin/bsdtar
mkdir -p /data/src && cd /data/src
touch "$(printf "raw\377\376name")" "$(printf "caf\303\251")" "$(printf "\346\227\245\346\234\254\350\252\236")" "$(printf "emoji\360\237\230\200")" "$(printf "latin1\351")"
mkdir -p "$(printf "d\377ir")"; printf data > "$(printf "d\377ir/f\377")"
names() { (cd "$1" && find . -print0 | LC_ALL=C sort -z | sha256sum | cut -c1-16); }
want=$(names /data/src)
trial() {
  label="$1"; create_opt="$2"; extract_opt="$3"; shift 3
  rm -rf /data/dst; mkdir -p /data/dst
  env "$@" $B --create --file /data/x.tar --format pax --numeric-owner $create_opt -C /data/src -- . 2>/data/ce; c=$?
  env "$@" $B --extract --file /data/x.tar --numeric-owner $extract_opt -C /data/dst 2>/data/ee; e=$?
  if [ "$(names /data/dst)" = "$want" ]; then r=equal; else r=DIFFERENT; fi
  quiet=no; [ ! -s /data/ce ] && [ ! -s /data/ee ] && quiet=yes
  echo "INFO: $label: create=$c extract=$e names=$r silent=$quiet stderr=$(cat /data/ce /data/ee | head -c 140 | tr "\n" " ")"
  [ $c -eq 0 ] && [ $e -eq 0 ] && [ $r = equal ] && [ $quiet = yes ]
}
trial "default, locale C" "" "" LC_ALL=C; echo "INFO: default_c_clean=$?"
trial "default, locale C.UTF-8" "" "" LC_ALL=C.UTF-8; echo "INFO: default_utf8_clean=$?"
trial "create hdrcharset=BINARY, locale C" "--options hdrcharset=BINARY" "" LC_ALL=C || echo "FAIL: BINARY on create, locale C"
trial "create hdrcharset=BINARY, locale C.UTF-8" "--options hdrcharset=BINARY" "" LC_ALL=C.UTF-8 || echo "FAIL: BINARY on create, locale C.UTF-8"
trial "hdrcharset=BINARY on create and extract, locale C" "--options hdrcharset=BINARY" "--options hdrcharset=BINARY" LC_ALL=C; echo "INFO: binary_on_extract_clean=$?"
'
  run_scripted_test 6 "open fault 5: raw-byte and Unicode names survive create then extract in the static musl build" "$script" $SEVEN_CAPS
}

test_8_special_entries() {
  local script='
set -u
B=/opt/sbx/bin/bsdtar
mkdir -p /data/src /data/dst && cd /data/src
printf one > "a b"
printf two > "$(printf "new\nline")"
printf three > "tab	name"
: > zero_byte
ln "a b" hard_one; ln "a b" hard_two
mkdir "dir with space"; printf x > "dir with space/in side"
names() { (cd "$1" && find . -printf "%p\t%y\t%s\t%n\t%m\0" | LC_ALL=C sort -z | tr "\0" "\n" | sed "s/\t/|/g" | tr "\n" ";"); }
before=$(names /data/src)
$B --create --file - --format pax --numeric-owner --no-xattrs --no-acls --no-fflags -C /data/src -- . | $B --extract --file - --preserve-permissions --same-owner --numeric-owner --no-xattrs --no-acls --no-fflags -C /data/dst
echo "INFO: exit=$?"
after=$(names /data/dst)
[ "$before" = "$after" ] || { echo "FAIL: listing differs"; echo "$before"; echo "$after"; }
[ /data/dst/hard_one -ef /data/dst/hard_two ] && [ "/data/dst/a b" -ef /data/dst/hard_one ] && echo "INFO: hard links share one inode after restore" || echo "FAIL: hard links lost"
[ -f /data/dst/zero_byte ] && [ ! -s /data/dst/zero_byte ] && echo "INFO: 0-byte file kept" || echo "FAIL: 0-byte file"
[ -f "/data/dst/$(printf "new\nline")" ] && echo "INFO: name with newline kept" || echo "FAIL: newline name"
[ -f "/data/dst/dir with space/in side" ] && echo "INFO: names with spaces kept" || echo "FAIL: space name"
'
  run_scripted_test 8 "names with space, tab and newline; hard links; 0-byte file" "$script" $SEVEN_CAPS
}
