#!/usr/bin/env bash

NON_ROOT_USER=10001:10001

test_9_non_root_same_owner() {
  local script='
set -u
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
echo ready
'
  local out
  out=$(probe_script "$script" -v "${DATA_VOL}:/data" $SEVEN_CAPS 2>&1)
  [ "$(tail -n 1 <<<"$out")" = ready ] || { record FAIL 9 "non-root same-owner" "set-up failed: $out"; return; }
  local flags=(--extract --file - --preserve-permissions --same-owner --numeric-owner)
  local detail="" label tarname dir res verdict=PASS
  for variant in "all.tar o9a same-owner" "mine.tar o9b same-owner" "all.tar o9c no-same-owner"; do
    set -- $variant
    tarname=$1 dir=$2 label=$3
    local args=("${flags[@]}")
    if [ "$label" = no-same-owner ]; then
      args=(--extract --file - --preserve-permissions --no-same-owner --numeric-owner)
    fi
    res=$(probe_run -i --user "$NON_ROOT_USER" --cap-drop ALL -v "${DATA_VOL}:/data" "$PROBE_IMAGE" \
      bash -c '/opt/sbx/bin/bsdtar "$@" -C /data/'"$dir"' < /data/'"$tarname"'; echo "exit=$?"; find /data/'"$dir"' -printf "%P:%U:%G:%m " ' _ "${args[@]}" 2>&1 </dev/null | tr '\n' ' ')
    detail="${detail}[${tarname} ${label}] ${res} "
    case "${label}:${res}" in
      same-owner:*"exit=1"*) [ "$tarname" = all.tar ] || verdict=FAIL ;;
      *"exit=0"*) ;;
      *) verdict=FAIL ;;
    esac
  done
  record "$verdict" 9 "bsdtar -x as uid 10001, no capability: --same-owner exits 1 for foreign ids only; own ids and --no-same-owner exit 0" "$detail"
}

test_10_read_only_root_second_container() {
  local name="${RUN_ID}-seeder" shared="${RUN_ID}-shared" verdict=PASS detail="" out
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
  out=$(docker exec -i "$name" /sbx/bin/bsdtar -x -f - --preserve-permissions --same-owner --numeric-owner -C /shared <"$tar_file" 2>&1)
  local code=$?
  detail="docker exec -i bsdtar -x into shared volume: exit=${code} stderr=${out:-none}; "
  [ "$code" = 0 ] && [ -z "$out" ] || verdict=FAIL
  out=$(docker run --rm --platform "$PLATFORM" -v "${shared}:/shared:ro" "$PROBE_IMAGE" find /shared -printf '%P:%U:%G:%m ' 2>&1)
  detail="${detail}shared volume holds: ${out}; "
  case "$out" in *"seed/data/a.csv:10001:10001:440"*"seed/run.sh:0:0:555"*|*"seed/run.sh:0:0:555"*"seed/data/a.csv:10001:10001:440"*) ;; *) verdict=FAIL ;; esac
  out=$(docker exec "$name" /sbx/bin/bsdtar -c -f /rootfs-write-test /shared 2>&1)
  detail="${detail}write to the read-only root: exit=$? ${out:0:80}; "
  docker run --rm --platform "$PLATFORM" --pid "container:${name}" "$PROBE_IMAGE" \
    bash -c '(sleep 2 & echo $! > /tmp/p); sleep 4; p=$(cat /tmp/p); if [ -e /proc/$p/status ]; then grep -m1 "^State:" /proc/$p/status; else echo gone; fi' >"${WORK_DIR}/orphan.txt" 2>&1
  detail="${detail}orphan of a joined container, 4 s later: $(tr '\n' ' ' <"${WORK_DIR}/orphan.txt"); "
  [ "$(tr -d '\n' <"${WORK_DIR}/orphan.txt")" = gone ] || verdict=FAIL
  local t0 t1 exit_code
  t0=$(now_ms)
  docker kill --signal TERM "$name" >/dev/null
  exit_code=$(docker wait "$name")
  t1=$(now_ms)
  detail="${detail}SIGTERM: exit=${exit_code} after $((t1 - t0)) ms; "
  [ "$exit_code" = 0 ] && [ $((t1 - t0)) -lt 1000 ] || verdict=FAIL
  docker rm -f "$name" >/dev/null
  docker volume rm "$shared" >/dev/null
  record "$verdict" 10 "read-only root, user 0, 7 caps: pause + docker exec -i bsdtar -x into a shared volume" "$detail"
}

test_10b_save_read_side_as_non_root() {
  local script='
set -u
rm -rf /data/rootdir; mkdir -p /data/rootdir/sub; cd /data/rootdir
printf a > a.txt; chmod 644 a.txt; printf b > sub/b.sh; chmod 755 sub/b.sh; printf s > secret.txt; chmod 600 secret.txt
mkdir private; chmod 700 private; printf p > private/p.txt
echo ready
'
  local out
  out=$(probe_script "$script" -v "${DATA_VOL}:/data" $SEVEN_CAPS 2>&1)
  [ "$(tail -n 1 <<<"$out")" = ready ] || { record FAIL 10b "save read side as non-root" "set-up failed: $out"; return; }
  local detail="" res
  res=$(probe_run --user "$NON_ROOT_USER" --cap-drop ALL -v "${DATA_VOL}:/data:ro" "$PROBE_IMAGE" \
    bash -c '/opt/sbx/bin/bsdtar --create --file /dev/null --format pax --numeric-owner -C /data -- rootdir/a.txt rootdir/sub; echo "exit=$?"' 2>&1 | tr '\n' ' ')
  detail="readable root-owned files and dir (0644, 0755): ${res}; "
  case "$res" in *"exit=0 "*) ;; *) detail="${detail}FAILED; " ;; esac
  res=$(probe_run --user "$NON_ROOT_USER" --cap-drop ALL -v "${DATA_VOL}:/data:ro" "$PROBE_IMAGE" \
    bash -c '/opt/sbx/bin/bsdtar --create --file /dev/null --format pax --numeric-owner -C /data -- rootdir; echo "exit=$?"' 2>&1 | tr '\n' ' ')
  detail="${detail}whole dir with a 0600 file and a 0700 dir of root: ${res}"
  if [[ "$detail" == *FAILED* ]]; then
    record FAIL 10b "bsdtar create on a root-owned directory as uid 10001 (read side of save)" "$detail"
  else
    record PASS 10b "bsdtar create on a root-owned directory as uid 10001 (read side of save)" "$detail"
  fi
}
