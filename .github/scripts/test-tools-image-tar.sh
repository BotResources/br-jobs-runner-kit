#!/usr/bin/env bash

TREE_SCRIPT='
set -u
B=/opt/sbx/bin/bsdtar
CREATE="$B $SAVE_FLAGS"
EXTRACT="$B $RESTORE_FLAGS"
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
info "entries=$entries tar_bytes=$(stat -c %s /data/save.tar) create_ms=$(( (t1 - t0) / 1000000 )) extract_ms=$(( (t2 - t1) / 1000000 ))"
echo "EXTRACT_RC=$extract_rc"; echo "EXTRACT_STDERR_BYTES=$(stat -c %s /data/extract.err)"
eq "create exit" "$create_rc" 0
eq "create stderr bytes" "$(stat -c %s /data/create.err)" 0
eq "extract exit" "$extract_rc" 0
eq "extract stderr bytes" "$(stat -c %s /data/extract.err)" 0
[ "$entries" -gt 1000 ] && pass "the tar holds more than 1000 entries" || fail "entries=$entries"
[ "$(wc -l < /data/before.txt)" -gt 3000 ] && pass "the manifest has more than 3000 lines" || fail "manifest too short"
if diff /data/before.txt /data/after.txt > /data/diff.txt; then
  pass "manifest equal after restore (mode, uid, gid, symlink target, file hash, mtime seconds, sub-second mtime)"
else
  fail "manifest differs: $(head -n 20 /data/diff.txt | tr "\n" " ")"
fi
'

test_3_n15_round_trip() {
  run_scripted_test 3 "tree round trip with the README save and restore lines: 1000 files, 50 MB file, symlinks, empty dirs, modes, uid/gid, mtime" \
    "$TREE_SCRIPT" $SEVEN_CAPS
}

test_4_capability_set() {
  local exit_code stderr_bytes
  exit_code=$(sed -n 's/^EXTRACT_RC=//p' "${WORK_DIR}/last_output")
  stderr_bytes=$(sed -n 's/^EXTRACT_STDERR_BYTES=//p' "${WORK_DIR}/last_output")
  if [ "${exit_code:-x}" = 0 ] && [ "${stderr_bytes:-x}" = 0 ]; then
    record PASS 4a "extraction as root with the 7-capability set: exit 0, no warning" "restore of test 3"
  else
    record FAIL 4a "extraction as root with the 7-capability set: exit 0, no warning" "exit=${exit_code:-unknown} stderr_bytes=${stderr_bytes:-unknown}"
  fi
  make_xattr_tar
  run_host_test 4b "xattr entry with the default flags: exit 1, a message, the file is written" test_4b_body
  run_host_test 4c "xattr entry with --no-xattrs --no-acls --no-fflags: exit 0, no output" test_4c_body
  run_host_test 4d "chown to an id unmapped in the user namespace: exit 1, a message, the file is written" test_4d_body
}
