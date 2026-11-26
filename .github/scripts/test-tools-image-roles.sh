#!/usr/bin/env bash

test_9_body() {
  local setup='
B=/opt/sbx/bin/bsdtar
rm -rf /data/in9 /data/o9a /data/o9b /data/o9c; mkdir -p /data/in9/work /data/o9a /data/o9b /data/o9c
cd /data/in9/work
printf mine > mine.txt; chown 10001:10001 mine.txt; chmod 640 mine.txt
printf other > other.txt; chown 20000:30000 other.txt; chmod 644 other.txt
printf root > rootfile.txt; chmod 600 rootfile.txt
mkdir sub; chown 10001:10001 sub
$B --create --file /data/all.tar --format pax --numeric-owner -C /data/in9 -- work
$B --create --file /data/mine.tar --format pax --numeric-owner -C /data/in9 -- work/mine.txt work/sub
chown -R 10001:10001 /data/o9a /data/o9b /data/o9c
pass "set-up done"
'
  local checks='
B=/opt/sbx/bin/bsdtar
NO_SAME_OWNER=${RESTORE_FLAGS/--same-owner/--no-same-owner}
eq "the --no-same-owner flags differ from the README flags" "$([ "$NO_SAME_OWNER" != "$RESTORE_FLAGS" ] && echo yes)" yes
eq "user" "$(id -u):$(id -g)" 10001:10001
$B $RESTORE_FLAGS -C /data/o9a </data/all.tar 2>/tmp/e9a; rc=$?
eq "--same-owner, a tar with foreign ids: exit" "$rc" 1
eq "--same-owner, a tar with foreign ids: one warning per foreign entry" "$(grep -c "Can.t set user=" /tmp/e9a)" 3
has "--same-owner, a tar with foreign ids: reason" "$(cat /tmp/e9a)" "Operation not permitted"
eq "--same-owner, a tar with foreign ids: entries written" "$(find /data/o9a | wc -l | tr -d " ")" 6
eq "--same-owner, a tar with foreign ids: entries not owned by 10001" "$(find /data/o9a ! -user 10001 | wc -l | tr -d " ")" 0
$B $RESTORE_FLAGS -C /data/o9b </data/mine.tar 2>/tmp/e9b; rc=$?
eq "--same-owner, a tar of own ids: exit" "$rc" 0
eq "--same-owner, a tar of own ids: stderr" "$(cat /tmp/e9b)" ""
eq "--same-owner, a tar of own ids: entries written" "$(find /data/o9b | wc -l | tr -d " ")" 4
$B $NO_SAME_OWNER -C /data/o9c </data/all.tar 2>/tmp/e9c; rc=$?
eq "--no-same-owner, a tar with foreign ids: exit" "$rc" 0
eq "--no-same-owner, a tar with foreign ids: stderr" "$(cat /tmp/e9c)" ""
eq "--no-same-owner, a tar with foreign ids: entries written" "$(find /data/o9c | wc -l | tr -d " ")" 6
eq "--no-same-owner, a tar with foreign ids: entries not owned by 10001" "$(find /data/o9c ! -user 10001 | wc -l | tr -d " ")" 0
eq "--no-same-owner keeps the modes" "$(find /data/o9c -name rootfile.txt -printf "%m")" 600
'
  probe_checks "$setup" -v "${DATA_VOL}:/data" $SEVEN_CAPS
  probe_checks "$checks" --user "$NON_ROOT_USER" --cap-drop ALL -v "${DATA_VOL}:/data"
}

test_9_non_root_same_owner() {
  run_host_test 9 "bsdtar -x as uid 10001, no capability: --same-owner exits 1 for foreign ids and writes every entry; own ids and --no-same-owner exit 0" test_9_body
}

test_10_body() {
  local name="${RUN_ID}-seeder" shared="${RUN_ID}-shared" out code listing result
  docker volume create "$shared" >/dev/null
  docker run -d --init=false --name "$name" --platform "$PLATFORM" --read-only --user 0 $SEVEN_CAPS \
    -v "${shared}:/shared" "$IMAGE" /sbx/bin/pause >/dev/null
  wait_for_container_state "$name" running
  local tar_file="${WORK_DIR}/seed.tar"
  python3 -I - "$tar_file" <<'PY'
import io, sys, tarfile
with tarfile.open(sys.argv[1], "w", format=tarfile.PAX_FORMAT) as t:
    for name, mode, uid in (("seed/readme.txt", 0o444, 0), ("seed/data/a.csv", 0o440, 10001), ("seed/run.sh", 0o555, 0)):
        body = b"seed " + name.encode() + b"\n"
        info = tarfile.TarInfo(name)
        info.size, info.mode, info.uid, info.gid = len(body), mode, uid, uid
        t.addfile(info, io.BytesIO(body))
PY
  out=$(docker exec -i "$name" /sbx/bin/bsdtar $RESTORE_FLAGS -C /shared <"$tar_file" 2>&1)
  code=$?
  eq "docker exec -i bsdtar -x into the shared volume: exit" "$code" 0
  eq "docker exec -i bsdtar -x into the shared volume: output" "$out" ""
  listing=$(docker run --rm --platform "$PLATFORM" -v "${shared}:/shared:ro" "$PROBE_IMAGE" find /shared -type f -printf '%P:%U:%G:%m\n' 2>&1 | sort | tr '\n' ' ')
  eq "files, owners and modes in the shared volume" "$listing" "seed/data/a.csv:10001:10001:440 seed/readme.txt:0:0:444 seed/run.sh:0:0:555 "
  docker exec "$name" /sbx/bin/bsdtar -c -f /shared/control.tar /shared/seed/readme.txt >/dev/null 2>&1
  eq "control: a write to the shared volume works" "$?" 0
  out=$(docker exec "$name" /sbx/bin/bsdtar -c -f /rootfs-write-test /shared/seed/readme.txt 2>&1)
  code=$?
  ne0 "a write to the read-only root file system fails" "$code"
  has "the write is refused at the open of the file" "$out" "Failed to open '/rootfs-write-test'"
  out=$(docker run --rm --platform "$PLATFORM" --pid "container:${name}" "$PROBE_IMAGE" \
    bash -c '(sleep 2 & echo $! > /tmp/p); sleep 4; p=$(cat /tmp/p); if [ -e /proc/$p/status ]; then grep -m1 "^State:" /proc/$p/status; else echo gone; fi' 2>&1)
  eq "an orphan of a container in the pid namespace of pause is reaped" "$out" gone
  result=$(exit_after_signal "$name" TERM)
  eq "SIGTERM ends pause with exit 0" "${result% *}" 0
  info "SIGTERM: ${result#* } ms"
  docker rm -f "$name" >/dev/null
  docker volume rm "$shared" >/dev/null
}

test_10_read_only_root_second_container() {
  run_host_test 10 "read-only root, user 0, 7 caps: pause + docker exec -i bsdtar -x into a shared volume" test_10_body
}

test_10b_body() {
  local setup='
rm -rf /data/rootdir; mkdir -p /data/rootdir/sub /data/rootdir_secret; cd /data/rootdir
printf a > a.txt; chmod 644 a.txt; printf b > sub/b.sh; chmod 755 sub/b.sh
printf s > /data/rootdir_secret/secret.txt; chmod 600 /data/rootdir_secret/secret.txt
mkdir /data/rootdir_private; chmod 700 /data/rootdir_private; printf p > /data/rootdir_private/p.txt
pass "set-up done"
'
  local checks='
B=/opt/sbx/bin/bsdtar
$B $SAVE_FLAGS -C /data -- rootdir/a.txt rootdir/sub >/dev/null 2>/tmp/e1; rc=$?
eq "readable root-owned files and directory (0644, 0755): exit" "$rc" 0
eq "readable root-owned files and directory: stderr" "$(cat /tmp/e1)" ""
$B $SAVE_FLAGS -C /data -- rootdir_secret >/dev/null 2>/tmp/e2; rc=$?
eq "directory with a 0600 file of root: exit" "$rc" 1
has "directory with a 0600 file of root: message" "$(cat /tmp/e2)" "Permission denied"
$B $SAVE_FLAGS -C /data -- rootdir_private >/dev/null 2>/tmp/e3; rc=$?
eq "0700 directory of root: exit" "$rc" 1
has "0700 directory of root: message" "$(cat /tmp/e3)" "Permission denied"
'
  probe_checks "$setup" -v "${DATA_VOL}:/data" $SEVEN_CAPS
  probe_checks "$checks" --user "$NON_ROOT_USER" --cap-drop ALL -v "${DATA_VOL}:/data:ro"
}

test_10b_save_read_side_as_non_root() {
  run_host_test 10b "bsdtar create on a root-owned directory as uid 10001 (read side of save)" test_10b_body
}

test_10c_body() {
  local setup='
rm -rf /data/one10c /data/out10c /data/one10c.tar /data/two10c.tar
mkdir -p /data/one10c/work /data/out10c
printf alpha > /data/one10c/work/a.txt; chmod 640 /data/one10c/work/a.txt
printf beta > /data/one10c/work/b.txt; chmod 600 /data/one10c/work/b.txt
pass "set-up done"
'
  local checks='
B=/opt/sbx/bin/bsdtar
$B $SAVE_FLAGS -C /data/one10c -- work/a.txt >/data/one10c.tar 2>/tmp/e1; rc=$?
eq "create of one path: exit" "$rc" 0
eq "create of one path: stderr" "$(cat /tmp/e1)" ""
eq "create of one path: the tar holds that one entry" "$($B -t -f /data/one10c.tar | tr "\n" " ")" "work/a.txt "
$B $SAVE_FLAGS -C /data/one10c -- work >/data/two10c.tar 2>/tmp/e2; rc=$?
eq "create of the directory: exit" "$rc" 0
eq "create of the directory: the tar holds the directory and the two files" "$($B -t -f /data/two10c.tar | LC_ALL=C sort | tr "\n" " ")" "work/ work/a.txt work/b.txt "
$B $RESTORE_FLAGS -C /data/out10c work/b.txt </data/two10c.tar >/tmp/o3 2>/tmp/e3; rc=$?
eq "extract of one entry: exit" "$rc" 0
eq "extract of one entry: stdout" "$(cat /tmp/o3)" ""
eq "extract of one entry: stderr" "$(cat /tmp/e3)" ""
eq "extract of one entry: only that file is written" "$(find /data/out10c -mindepth 1 | LC_ALL=C sort | tr "\n" " ")" "/data/out10c/work /data/out10c/work/b.txt "
eq "extract of one entry: content" "$(cat /data/out10c/work/b.txt)" beta
eq "extract of one entry: mode" "$(stat -c %a /data/out10c/work/b.txt)" 600
'
  probe_checks "$setup" -v "${DATA_VOL}:/data" $SEVEN_CAPS
  probe_checks "$checks" -v "${DATA_VOL}:/data" $SEVEN_CAPS
}

test_10c_one_file_each_way() {
  run_host_test 10c "one file read by create of one path and one file written by extract of one entry, with the output checked" test_10c_body
}
