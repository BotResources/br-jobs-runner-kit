#!/usr/bin/env bash
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/common.sh"
scenario_begin s06-seed-read-only

seed_tar() {
  python3 - << 'PY'
import io
import sys
import tarfile

files = {"seed/hello.txt": b"seed-content\n", "seed/docs/readme.txt": b"readable\n", "seed/docs/data.bin": bytes(range(256)) * 64}
buffer = io.BytesIO()
with tarfile.open(fileobj=buffer, mode="w") as archive:
    for name, content in files.items():
        entry = tarfile.TarInfo(name)
        entry.size, entry.mode = len(content), 0o644
        archive.addfile(entry, io.BytesIO(content))
sys.stdout.buffer.write(buffer.getvalue())
PY
}

ns=$(ns_of "$CLIENT_A" "$NET_NONE")
wait_warm "$ns" 1 120 || { check seed-warm-pod fail "no warm pod in $ns"; scenario_end; }
claim_to pod "$ns" "seed-$(new_id)" agent-a || { check seed-claim fail "claim refused"; scenario_end; }

verdict seed-container-exists "container $SEED_CONTAINER in the pod" \
  test -n "$(k -n "$ns" get pod "$pod" -o json | jq -r --arg c "$SEED_CONTAINER" '.spec.containers[] | select(.name == $c) | .name')"
verdict seed-agent-not-root "uid of the agent: $(sx "$ns" "$pod" id -u)" test "$(sx "$ns" "$pod" id -u)" != 0

seed_tar | k -n "$ns" exec -i "$pod" -c "$SEED_CONTAINER" -- "$SEED_BSDTAR" -x --no-same-owner -f - --strip-components=1 -C "$SEED_PATH"
verdict seed-readable "agent reads $SEED_PATH/hello.txt" test "$(sx "$ns" "$pod" cat "$SEED_PATH/hello.txt")" = seed-content
verdict seed-file-count "three files under $SEED_PATH" \
  test "$(sx "$ns" "$pod" find "$SEED_PATH" -type f | count_lines)" = 3
verdict seed-mount-read-only "agent mount of $SEED_PATH is ro" \
  sx "$ns" "$pod" sh -c "grep -E ' $SEED_PATH .* ro[ ,]' /proc/mounts"

refused() {
  local out
  out=$(sx "$ns" "$pod" sh -c "$1" 2>&1) && return 1
  printf '%s\n' "$out" | grep -q -i -E 'read-only|permission denied|not permitted|superuser'
}
verdict seed-write-new-file-refused "agent creates a file in the seed" refused "echo x > $SEED_PATH/new.txt"
verdict seed-append-refused "agent appends to a seed file" refused "echo x >> $SEED_PATH/hello.txt"
verdict seed-delete-refused "agent deletes a seed file" refused "rm $SEED_PATH/hello.txt"
verdict seed-mkdir-refused "agent makes a directory in the seed" refused "mkdir $SEED_PATH/newdir"
verdict seed-chmod-refused "agent changes the mode of a seed file" refused "chmod 777 $SEED_PATH/hello.txt"
verdict seed-remount-refused "agent remounts the seed read-write" refused "mount -o remount,rw $SEED_PATH"

verdict seed-unchanged "seed content after the refused writes" test "$(sx "$ns" "$pod" cat "$SEED_PATH/hello.txt")" = seed-content
verdict seed-work-writable "positive control: /work accepts a write" sx "$ns" "$pod" sh -c 'echo ok > /work/seed-control'

scenario_end
